use std::collections::HashSet;

use libxml::{
    tree::{Document, Node, NodeType},
    xpath::Context,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Response,
};
use tokio::fs::DirEntry;

use super::constants;

use thiserror::Error;

use url::Url;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error")]
    IO(#[from] std::io::Error),
    #[error("Unknown Error")]
    Unknown,
}

#[derive(Error, Debug)]
pub enum FullTextParserError {
    #[error("libXml Error")]
    Xml,
    #[error("No content found")]
    Scrape,
    #[error("Url Error")]
    Url(#[from] url::ParseError),
    #[error("Http request failed")]
    Http,
    #[error("Config Error")]
    Config,
    #[error("IO Error")]
    IO,
    #[error("Content-type suggest no html")]
    ContentType,
    #[error("Invalid UTF8 Text")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Readability Error")]
    Readability,
    #[error("Unknown Error")]
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Replace {
    pub to_replace: String,
    pub replace_with: String,
}

#[derive(Clone, Debug)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigEntry {
    pub xpath_title: Vec<String>,
    pub xpath_author: Vec<String>,
    pub xpath_date: Vec<String>,
    pub xpath_body: Vec<String>,
    pub xpath_strip: Vec<String>,
    pub strip_id_or_class: Vec<String>,
    pub strip_image_src: Vec<String>,
    pub replace: Vec<Replace>,
    pub header: Vec<Header>,
    pub single_page_link: Option<String>,
    pub next_page_link: Option<String>,
}

pub struct Util;

impl Util {
    pub fn check_extension(path: &DirEntry, extension: &str) -> bool {
        if let Some(ext) = path.path().extension() {
            ext.to_str() == Some(extension)
        } else {
            false
        }
    }

    pub fn str_extract_value<'a>(identifier: &str, line: &'a str) -> &'a str {
        let value = &line[identifier.len()..];
        let value = value.trim();
        match value.find('#') {
            Some(pos) => &value[..pos],
            None => value,
        }
    }

    pub fn split_values(values: &str) -> Vec<&str> {
        values.split('|').map(|s| s.trim()).collect()
    }

    pub fn select_rule<'a>(
        site_specific_rule: Option<&'a str>,
        global_rule: Option<&'a str>,
    ) -> Option<&'a str> {
        if site_specific_rule.is_some() {
            site_specific_rule
        } else {
            global_rule
        }
    }

    pub fn generate_headers(
        site_specific_rule: Option<&ConfigEntry>,
        global_rule: &ConfigEntry,
    ) -> Result<HeaderMap, FullTextParserError> {
        let mut headers = HeaderMap::new();

        if let Some(config) = site_specific_rule {
            for header in &config.header {
                let name = HeaderName::from_bytes(header.name.as_bytes())
                    .map_err(|_| FullTextParserError::Config)?;
                let value = header
                    .value
                    .parse::<HeaderValue>()
                    .map_err(|_| FullTextParserError::Config)?;
                headers.insert(name, value);
            }
        }

        for header in &global_rule.header {
            let name = HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| FullTextParserError::Config)?;
            let value = header
                .value
                .parse::<HeaderValue>()
                .map_err(|_| FullTextParserError::Config)?;
            headers.insert(name, value);
        }

        Ok(headers)
    }

    pub fn find_page_url(xpath_ctx: &Context, xpath_page_link: &str) -> Option<url::Url> {
        let res = Self::evaluate_xpath(xpath_ctx, xpath_page_link, false).ok()?;
        let mut url = None;

        for node in res {
            let content = node.get_content();
            let url_str = if content.trim().is_empty() && node.get_attribute("href").is_some() {
                node.get_attribute("href")
                    .expect("already checked for href")
            } else {
                content
            };

            if let Ok(parsed_url) = url::Url::parse(&url_str) {
                url = Some(parsed_url);
                break;
            }
        }

        url
    }

    pub fn evaluate_xpath(
        xpath_ctx: &Context,
        xpath: &str,
        thorw_if_empty: bool,
    ) -> Result<Vec<Node>, FullTextParserError> {
        let res = xpath_ctx.evaluate(xpath).map_err(|()| {
            log::debug!("Evaluation of xpath '{}' yielded no results", xpath);
            FullTextParserError::Xml
        })?;

        let node_vec = res.get_nodes_as_vec();

        if node_vec.is_empty() {
            log::debug!("Evaluation of xpath '{}' yielded no results", xpath);
            if thorw_if_empty {
                return Err(FullTextParserError::Xml);
            }
        }

        Ok(node_vec)
    }

    pub fn check_content_type(response: &Response) -> Result<bool, FullTextParserError> {
        if response.status().is_success() {
            if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
                if let Ok(content_type) = content_type.to_str() {
                    if content_type.contains("text/html") {
                        return Ok(true);
                    }
                }
            }

            log::error!("Content type is not text/HTML");
            return Ok(false);
        }

        log::error!("Failed to determine content type");
        Err(FullTextParserError::Http)
    }

    pub fn check_redirect(response: &Response, original_url: &url::Url) -> Option<url::Url> {
        if response.status() == reqwest::StatusCode::PERMANENT_REDIRECT {
            log::debug!("Article url redirects to '{}'", response.url().as_str());
            return Some(response.url().clone());
        } else if response.url() != original_url {
            return Some(response.url().clone());
        }

        None
    }

    pub fn get_attribute(
        context: &Context,
        xpath: &str,
        attribute: &str,
    ) -> Result<String, FullTextParserError> {
        Util::evaluate_xpath(context, xpath, false)?
            .iter()
            .find_map(|node| node.get_attribute(attribute))
            .ok_or(FullTextParserError::Xml)
    }

    pub fn extract_value(context: &Context, xpath: &str) -> Result<String, FullTextParserError> {
        let node_vec = Util::evaluate_xpath(context, xpath, false)?;
        if let Some(val) = node_vec.get(0) {
            return Ok(val.get_content());
        }

        Err(FullTextParserError::Xml)
    }

    pub fn extract_value_merge(
        context: &Context,
        xpath: &str,
    ) -> Result<String, FullTextParserError> {
        let node_vec = Util::evaluate_xpath(context, xpath, true)?;
        let mut val = String::new();
        for node in node_vec {
            let part = node
                .get_content()
                .split_whitespace()
                .map(|s| format!("{} ", s))
                .collect::<String>();
            val.push_str(&part);
            val.push(' ');
        }

        Ok(val.trim().to_string())
    }

    pub fn strip_node(context: &Context, xpath: &str) -> Result<(), FullTextParserError> {
        let node_vec = Util::evaluate_xpath(context, xpath, false)?;
        let node_vec_clone = node_vec.clone();

        for mut node in node_vec {
            let tag_name = node.get_name();
            if constants::EMBED_TAG_NAMES.contains(tag_name.to_uppercase().as_str())
                && node
                    .get_attributes()
                    .iter()
                    .any(|(_name, value)| constants::VIDEOS.is_match(value))
            {
                continue;
            }

            if Self::parent_part_of_result(&node, &node_vec_clone) {
                continue;
            }

            node.unlink();
        }
        Ok(())
    }

    fn parent_part_of_result(node: &Node, xpath_result: &[Node]) -> bool {
        if let Some(parent) = node.get_parent() {
            for n in xpath_result {
                if n == &parent {
                    return true;
                }
            }

            return Self::parent_part_of_result(&parent, xpath_result);
        }

        false
    }

    pub fn strip_id_or_class(
        context: &Context,
        id_or_class: &str,
    ) -> Result<(), FullTextParserError> {
        let xpath = &format!(
            "//*[contains(@class, '{}') or contains(@id, '{}')]",
            id_or_class, id_or_class
        );

        let mut ancestor = xpath.clone();
        if ancestor.starts_with("//") {
            ancestor = ancestor.chars().skip(2).collect();
        }

        let query = &format!("{}[not(ancestor::{})]", xpath, ancestor);
        let node_vec = Util::evaluate_xpath(context, query, false)?;
        for mut node in node_vec {
            node.unlink();
        }
        Ok(())
    }

    pub fn is_probably_visible(node: &Node) -> bool {
        let is_hidden = node.get_attribute("hidden").is_some();
        let aria_hidden = node
            .get_attribute("aria-hidden")
            .map(|attr| attr == "true")
            .unwrap_or(false);
        let has_fallback_image = node
            .get_class_names()
            .iter()
            .any(|class| class.contains("fallback-image"));

        !is_hidden && !aria_hidden || has_fallback_image
    }

    pub fn is_whitespace(node: &Node) -> bool {
        let content = node.get_content();
        let tag_name = node.get_name().to_uppercase();

        let is_text_node = node
            .get_type()
            .map(|t| t == NodeType::TextNode)
            .unwrap_or(false);
        let is_element_node = node
            .get_type()
            .map(|t| t == NodeType::ElementNode)
            .unwrap_or(false);

        (is_text_node && content.trim().is_empty()) || (is_element_node && tag_name == "BR")
    }

    pub fn remove_and_next(node: &mut Node) -> Option<Node> {
        let next_node = Self::next_node(node, true);
        node.unlink();
        next_node
    }

    pub fn next_node(node: &Node, ignore_self_and_kids: bool) -> Option<Node> {
        let mut node = node.clone();

        // First check for kids if those aren't being ignored
        let first_child = node.get_first_child();
        if !ignore_self_and_kids && first_child.is_some() {
            return first_child;
        }

        // Then for siblings...
        let next_sibling = node.get_next_sibling();
        if next_sibling.is_some() {
            return next_sibling;
        }

        // And finally, move up the parent chain *and* find a sibling
        // (because this is depth-first traversal, we will have already
        // seen the parent nodes themselves).
        while let Some(parent) = node.get_parent() {
            let parent_name = parent.get_name().to_uppercase();
            if parent_name == "HTML" {
                break;
            }

            let next_sibling = parent.get_next_sibling();
            if next_sibling.is_some() {
                return next_sibling;
            } else {
                node = parent;
            }
        }

        None
    }

    pub fn get_inner_text(node: &Node, normalize_spaces: bool) -> String {
        let content = node.get_content().trim().to_owned();
        if normalize_spaces {
            constants::NORMALIZE.replace_all(&content, " ").into()
        } else {
            content
        }
    }

    pub fn text_similarity(a: &str, b: &str) -> f64 {
        let a = a.to_lowercase();
        let b = b.to_lowercase();
        let tokens_a = constants::TOKENIZE
            .split(&a)
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        let tokens_b = constants::TOKENIZE
            .split(&b)
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens_a.is_empty() || tokens_b.is_empty() {
            return 0.0;
        }

        let tokens_b_total = tokens_b.join(" ").len() as f64;
        let uniq_tokens_b = tokens_b
            .into_iter()
            .filter(|token| !tokens_a.iter().any(|t| t == token))
            .collect::<Vec<_>>();
        let uniq_tokens_b_total = uniq_tokens_b.join(" ").len() as f64;

        let distance_b = uniq_tokens_b_total / tokens_b_total;
        1.0 - distance_b
    }

    // Check if this node is an H1 or H2 element whose content is mostly
    // the same as the article title.
    pub fn header_duplicates_title(node: &Node, title: Option<&str>) -> bool {
        let name = node.get_name().to_lowercase();
        if name != "h1" && name != "h2" {
            return false;
        }
        let heading = Util::get_inner_text(node, false);

        if let Some(title) = title {
            Util::text_similarity(title, &heading) > 0.75
        } else {
            false
        }
    }

    pub fn has_any_descendent_tag(node: &Node, tag_names: &HashSet<&str>) -> bool {
        let children = node.get_child_elements();
        let is_direct_child = children
            .iter()
            .map(|node| node.get_name().to_uppercase())
            .any(|name| tag_names.contains(name.as_str()));

        if is_direct_child {
            return true;
        }

        for child in children {
            if Util::has_any_descendent_tag(&child, tag_names) {
                return true;
            }
        }

        false
    }

    pub fn has_ancestor_tag<F>(
        node: &Node,
        tag_name: &str,
        max_depth: Option<u64>,
        filter: Option<F>,
    ) -> bool
    where
        F: Fn(&Node) -> bool,
    {
        let max_depth = max_depth.unwrap_or(3);
        let tag_name = tag_name.to_uppercase();
        let mut depth = 0;
        let mut node = node.get_parent();

        loop {
            if depth > max_depth {
                return false;
            }

            let tmp_node = match node {
                Some(node) => node,
                None => return false,
            };

            if tmp_node.get_name().to_uppercase() == tag_name
                && filter
                    .as_ref()
                    .map(|filter| filter(&tmp_node))
                    .unwrap_or(true)
            {
                return true;
            }

            node = tmp_node.get_parent();
            depth += 1;
        }
    }

    pub fn has_single_tag_inside_element(node: &Node, tag: &str) -> bool {
        // There should be exactly 1 element child with given tag
        if node.get_child_elements().len() != 1
            || node
                .get_child_elements()
                .first()
                .map(|n| n.get_name().to_uppercase() != tag)
                .unwrap_or(false)
        {
            return false;
        }

        // And there should be no text nodes with real content
        !node.get_child_nodes().iter().any(|n| {
            n.get_type()
                .map(|t| t == NodeType::TextNode)
                .unwrap_or(false)
                && constants::HAS_CONTENT.is_match(&n.get_content())
        })
    }

    pub fn is_element_without_content(node: &Node) -> bool {
        if let Some(node_type) = node.get_type() {
            let len = node.get_child_nodes().len();

            node_type == NodeType::ElementNode
                && (len == 0
                    || len
                        == Self::get_elements_by_tag_name(node, "br").len()
                            + Self::get_elements_by_tag_name(node, "hr").len())
                && node.get_content().trim().is_empty()
        } else {
            false
        }
    }

    pub fn is_element_without_children(node: &Node) -> bool {
        if let Some(node_type) = node.get_type() {
            let len = node.get_child_nodes().len();
            node_type == NodeType::ElementNode
                && (len == 0 || node.get_content().trim().is_empty())
                && Self::get_elements_by_tag_names(node, &constants::VALID_EMPTY_TAGS).is_empty()
        } else {
            false
        }
    }

    pub fn get_elements_by_tag_names(node: &Node, tags: &HashSet<&str>) -> Vec<Node> {
        let mut vec = Vec::new();

        fn get_elems(node: &Node, tags: &HashSet<&str>, vec: &mut Vec<Node>) {
            for child in node.get_child_elements() {
                if tags.contains(child.get_name().to_uppercase().as_str()) {
                    vec.push(child.clone());
                }
                get_elems(&child, tags, vec);
            }
        }

        get_elems(node, tags, &mut vec);
        vec
    }

    pub fn get_elements_by_tag_name(node: &Node, tag: &str) -> Vec<Node> {
        let tag = tag.to_uppercase();
        let all_tags = tag == "*";
        let mut vec = Vec::new();

        fn get_elems(node: &Node, tag: &str, vec: &mut Vec<Node>, all_tags: bool) {
            for child in node.get_child_elements() {
                if all_tags || child.get_name().to_uppercase() == tag {
                    vec.push(child.clone());
                }
                get_elems(&child, tag, vec, all_tags);
            }
        }

        get_elems(node, &tag, &mut vec, all_tags);
        vec
    }

    pub fn get_link_density(node: &Node) -> f64 {
        let text_length = Util::get_inner_text(node, true).len();
        if text_length == 0 {
            return 0.0;
        }

        let mut link_length = 0.0;

        // XXX implement _reduceNodeList?
        let link_nodes = Util::get_elements_by_tag_name(node, "A");
        for link_node in link_nodes {
            if let Some(href) = link_node.get_attribute("href") {
                let coefficient = if constants::HASH_URL.is_match(&href) {
                    0.3
                } else {
                    1.0
                };
                link_length += Util::get_inner_text(&link_node, true).len() as f64 * coefficient;
            }
        }

        link_length / text_length as f64
    }

    // Determine whether element has any children block level elements.
    pub fn has_child_block_element(node: &Node) -> bool {
        node.get_child_nodes().iter().any(|node| {
            constants::DIV_TO_P_ELEMS.contains(node.get_name().to_uppercase().as_str())
                || Self::has_child_block_element(node)
        })
    }

    pub fn get_node_ancestors(node: &Node, max_depth: Option<u64>) -> Vec<Node> {
        let mut ancestors = Vec::new();
        let mut node = node.clone();
        let max_depth = max_depth.unwrap_or(u64::MAX);

        for _ in 0..max_depth {
            let parent = node.get_parent();
            match parent {
                Some(parent) => {
                    ancestors.push(parent.clone());
                    node = parent;
                }
                None => return ancestors,
            }
        }

        ancestors
    }

    pub fn has_tag_name(node: Option<&Node>, tag_name: &str) -> bool {
        node.map(|n| n.get_name().to_uppercase() == tag_name.to_uppercase())
            .unwrap_or(false)
    }

    // Check if node is image, or if node contains exactly only one image
    // whether as a direct child or as its descendants.
    pub fn is_single_image(node: &Node) -> bool {
        if node.get_name().to_uppercase() == "IMG" {
            true
        } else if node.get_child_elements().len() != 1 || node.get_content().trim() != "" {
            false
        } else if let Some(first_child) = node.get_child_elements().first() {
            Self::is_single_image(first_child)
        } else {
            false
        }
    }

    pub fn clean_headers(root: &mut Node) {
        let mut nodes = Util::get_elements_by_tag_name(root, "h1");
        nodes.append(&mut Util::get_elements_by_tag_name(root, "h2"));

        for mut node in nodes.into_iter().rev() {
            if Util::get_class_weight(&node) < 0 {
                log::debug!(
                    "Removing header with low class weight: {} {}",
                    node.get_name(),
                    node.get_attribute("class").unwrap_or_default()
                );
                node.unlink();
            }
        }
    }

    pub fn replace_schema_org_orbjects(root: &mut Node) {
        let nodes = Util::get_elements_by_tag_name(root, "div");

        for mut node in nodes.into_iter().rev() {
            if let Some(video_object) = VideoObject::parse_node(&node) {
                _ = video_object.replace(&mut node);
            } else if let Some(image_object) = ImageObject::parse_node(&node) {
                _ = image_object.replace(&mut node);
            }
        }
    }

    // Clean an element of all tags of type "tag" if they look fishy.
    // "Fishy" is an algorithm based on content length, classnames, link density, number of images & embeds, etc.
    pub fn clean_conditionally(root: &mut Node, tag: &str) {
        // Gather counts for other typical elements embedded within.
        // Traverse backwards so we can remove nodes at the same time
        // without effecting the traversal.
        //
        // TODO: Consider taking into account original contentScore here.
        let nodes = Util::get_elements_by_tag_name(root, tag);

        for mut node in nodes.into_iter().rev() {
            if Self::should_remove(&node, tag) {
                node.unlink();
            }
        }
    }

    fn should_remove(node: &Node, tag: &str) -> bool {
        // First check if this node IS data table, in which case don't remove it.
        let mut is_list = tag == "ul" || tag == "ol";
        if !is_list {
            let mut list_length = 0.0;
            let ul_nodes = Self::get_elements_by_tag_name(node, "ul");
            let ol_nodes = Self::get_elements_by_tag_name(node, "ol");
            for list_node in ul_nodes {
                list_length += Util::get_inner_text(&list_node, false).len() as f64;
            }
            for list_node in ol_nodes {
                list_length += Util::get_inner_text(&list_node, false).len() as f64;
            }
            is_list = (list_length / Util::get_inner_text(node, false).len() as f64) > 0.9;
        }

        if tag == "table" && Self::is_data_table(node) {
            return false;
        }

        // Next check if we're inside a data table, in which case don't remove it as well.
        if Self::has_ancestor_tag(node, "table", Some(u64::MAX), Some(Self::is_data_table)) {
            return false;
        }

        if Self::has_ancestor_tag(node, "code", None, None::<fn(&Node) -> bool>) {
            return false;
        }

        let weight = Self::get_class_weight(node);
        if weight < 0 {
            return true;
        }

        if Self::get_char_count(node, ',') < 10 {
            // If there are not very many commas, and the number of
            // non-paragraph elements is more than paragraphs or other
            // ominous signs, remove the element.
            let p = Self::get_elements_by_tag_name(node, "p").len();
            let img = Self::get_elements_by_tag_name(node, "img").len();
            let li = Self::get_elements_by_tag_name(node, "li").len() as i64 - 100;
            let input = Self::get_elements_by_tag_name(node, "input").len();
            let heading_density =
                Self::get_text_density(node, &["h1", "h2", "h3", "h4", "h5", "h6"]);

            let mut embed_count = 0;
            let embed_tags = ["object", "embed", "iframe"];

            for embed_tag in embed_tags {
                for embed_node in Self::get_elements_by_tag_name(node, embed_tag) {
                    // If this embed has attribute that matches video regex, don't delete it.
                    for (_name, value) in embed_node.get_attributes() {
                        if constants::VIDEOS.is_match(&value) {
                            return false;
                        }
                    }

                    // For embed with <object> tag, check inner HTML as well.
                    // if embed_node.get_name().to_lowercase() == "object" && constants::VIDEOS.is_match(embed_node.innerHTML) {
                    //     return false;
                    // }

                    embed_count += 1;
                }
            }

            let link_density = Self::get_link_density(node);
            let content = Self::get_inner_text(node, true);
            let content_length = content.len();
            let has_figure_ancestor =
                Self::has_ancestor_tag(node, "figure", None, None::<fn(&Node) -> bool>);

            let image_obj_count = Util::get_elements_by_tag_name(node, "imageobject").len();
            let video_obj_count = Util::get_elements_by_tag_name(node, "videoobject").len();

            if image_obj_count > 0 || video_obj_count > 0 {
                return false;
            }

            let have_to_remove = (img > 1 && (p as f64 / img as f64) < 0.5 && !has_figure_ancestor)
                || (!is_list && li > p as i64)
                || (input as f64 > f64::floor(p as f64 / 3.0))
                || (!is_list
                    && heading_density < 0.9
                    && content_length < 25
                    && (img == 0 || img > 2)
                    && !has_figure_ancestor)
                || (!is_list && weight < 25 && link_density > 0.2)
                || (weight >= 25 && link_density > 0.5)
                || ((embed_count == 1 && content_length < 75) || embed_count > 1);

            // Allow simple lists of images to remain in pages
            if is_list && have_to_remove {
                for child in node.get_child_elements() {
                    // Don't filter in lists with li's that contain more than one child
                    if child.get_child_elements().len() > 1 {
                        return have_to_remove;
                    }
                }

                let li_count = Util::get_elements_by_tag_name(node, "li").len();
                // Only allow the list to remain if every li contains an image
                if img == li_count {
                    return false;
                }
            }

            have_to_remove
        } else {
            false
        }
    }

    pub fn get_class_weight(node: &Node) -> i64 {
        let mut weight = 0;

        // Look for a special classname
        if let Some(class_names) = node.get_property("class") {
            if constants::NEGATIVE.is_match(&class_names) {
                weight -= 25;
            }

            if constants::POSITIVE.is_match(&class_names) {
                weight += 25;
            }
        }

        // Look for a special ID
        if let Some(class_names) = node.get_property("id") {
            if constants::NEGATIVE.is_match(&class_names) {
                weight -= 25;
            }

            if constants::POSITIVE.is_match(&class_names) {
                weight += 25;
            }
        }

        weight
    }

    fn get_char_count(node: &Node, char: char) -> usize {
        Util::get_inner_text(node, false).split(char).count() - 1
    }

    fn get_text_density(node: &Node, tags: &[&str]) -> f64 {
        let text_length = Util::get_inner_text(node, false).len();
        if text_length == 0 {
            return 0.0;
        }

        let mut children_length = 0;
        for tag in tags {
            for child in Self::get_elements_by_tag_name(node, tag) {
                children_length += Util::get_inner_text(&child, false).len()
            }
        }
        children_length as f64 / text_length as f64
    }

    fn is_data_table(node: &Node) -> bool {
        node.get_attribute(constants::DATA_TABLE_ATTR)
            .and_then(|is_data_table| is_data_table.parse::<bool>().ok())
            .unwrap_or(false)
    }

    pub fn mark_data_tables(context: &Context) -> Result<(), FullTextParserError> {
        let nodes = Util::evaluate_xpath(context, "//table", false)?;
        for mut node in nodes {
            if node
                .get_attribute("role")
                .map(|role| role == "presentation")
                .unwrap_or(false)
            {
                let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "false");
                continue;
            }

            if node
                .get_attribute("datatable")
                .map(|role| role == "0")
                .unwrap_or(false)
            {
                let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "false");
                continue;
            }

            if node.get_attribute("summary").is_some() {
                let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "true");
                continue;
            }

            if let Some(first_caption) = Self::get_elements_by_tag_name(&node, "caption").first() {
                if !first_caption.get_child_nodes().is_empty() {
                    let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "true");
                    continue;
                }
            }

            // If the table has a descendant with any of these tags, consider a data table:
            let data_table_descendants = ["col", "colgroup", "tfoot", "thead", "th"];
            for descendant in data_table_descendants {
                if !Self::get_elements_by_tag_name(&node, descendant).is_empty() {
                    let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "true");
                    continue;
                }
            }

            // Nested tables indicate a layout table:
            if !Self::get_elements_by_tag_name(&node, "table").is_empty() {
                let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "false");
                continue;
            }

            let (rows, columns) = Self::get_row_and_column_count(&node);
            if rows >= 10 || columns > 4 {
                let _ = node.set_attribute(constants::DATA_TABLE_ATTR, "true");
                continue;
            }

            // Now just go by size entirely:
            let _ = node.set_attribute(
                constants::DATA_TABLE_ATTR,
                if rows * columns > 10 { "true" } else { "false" },
            );
        }

        Ok(())
    }

    pub fn get_row_and_column_count(node: &Node) -> (usize, usize) {
        if node.get_name().to_uppercase() != "TABLE" {
            return (0, 0);
        }

        let mut rows = 0;
        let mut columns = 0;

        let trs = Self::get_elements_by_tag_name(node, "tr");
        for tr in trs {
            let row_span = tr
                .get_attribute("rowspan")
                .and_then(|span| span.parse::<usize>().ok())
                .unwrap_or(1);
            rows += row_span;

            // Now look for column-related info
            let mut columns_in_this_row = 0;
            let cells = Self::get_elements_by_tag_name(&tr, "td");
            for cell in cells {
                let colspan = cell
                    .get_attribute("colspan")
                    .and_then(|span| span.parse::<usize>().ok())
                    .unwrap_or(1);
                columns_in_this_row += colspan;
            }
            columns = usize::max(columns, columns_in_this_row);
        }

        (rows, columns)
    }

    pub fn is_phrasing_content(node: &Node) -> bool {
        let tag_name = node.get_name().to_uppercase();
        let is_text_node = node
            .get_type()
            .map(|t| t == NodeType::TextNode)
            .unwrap_or(false);

        is_text_node
            || constants::PHRASING_ELEMS.contains(&tag_name.as_str())
            || ((tag_name == "A" || tag_name == "DEL" || tag_name == "INS")
                && node.get_child_nodes().iter().all(Self::is_phrasing_content))
    }

    #[allow(dead_code)]
    pub fn serialize_node(node: &Node, filename: &str) {
        let mut doc = libxml::tree::Document::new().unwrap();
        doc.set_root_element(node);
        let html = doc.to_string_with_options(libxml::tree::SaveOptions {
            format: true,
            no_declaration: false,
            no_empty_tags: true,
            no_xhtml: false,
            xhtml: false,
            as_xml: false,
            as_html: true,
            non_significant_whitespace: false,
        });
        std::fs::write(filename, html).unwrap();
    }

    #[allow(dead_code)]
    pub fn serialize_document(doc: &Document, filename: &str) {
        let html = doc.to_string_with_options(libxml::tree::SaveOptions {
            format: true,
            no_declaration: false,
            no_empty_tags: true,
            no_xhtml: false,
            xhtml: false,
            as_xml: false,
            as_html: true,
            non_significant_whitespace: false,
        });
        std::fs::write(filename, html).unwrap();
    }

    // Replaces 2 or more successive <br> elements with a single <p>.
    // Whitespace between <br> elements are ignored.
    // For example:
    //   <div>foo<br>bar<br> <br><br>abc</div>
    // will become:
    //   <div>foo<br>bar<p>abc</p></div>
    pub fn replace_brs(node: &Node, document: &Document) {
        let br_nodes = Self::get_elements_by_tag_name(node, "br");

        for br_node in br_nodes {
            let mut next = br_node.get_next_sibling();

            // Whether 2 or more <br> elements have been found and replaced with a
            // <p> block.
            let mut replaced = false;

            // If we find a <br> chain, remove the <br>s until we hit another node
            // or non-whitespace. This leaves behind the first <br> in the chain
            // (which will be replaced with a <p> later).
            while let Some(mut n) = next {
                let is_text_whitespace = n
                    .get_type()
                    .map(|t| t == NodeType::TextNode)
                    .unwrap_or(false)
                    && n.get_content().trim().is_empty();
                let is_br_node = n.get_name().to_uppercase() == "BR";
                let next_is_br_node = n
                    .get_next_sibling()
                    .map(|n| n.get_name().to_uppercase() == "BR")
                    .unwrap_or(false);

                if !is_text_whitespace && !is_br_node {
                    break;
                }

                next = n.get_next_sibling();

                if is_br_node || (is_text_whitespace && next_is_br_node) {
                    replaced = true;
                    n.unlink();
                }
            }

            if !replaced {
                continue;
            }

            // If we removed a <br> chain, replace the remaining <br> with a <p>. Add
            // all sibling nodes as children of the <p> until we hit another <br>
            // chain.
            let mut parent = match br_node.get_parent() {
                Some(parent) => parent,
                None => continue,
            };
            let mut p = Node::new("p", None, document).unwrap();
            _ = parent.replace_child_node(p.clone(), br_node).unwrap();

            next = p.get_next_sibling();

            while let Some(mut next_node) = next {
                // If we've hit another <br><br>, we're done adding children to this <p>.
                if next_node.get_name().to_uppercase() == "BR" {
                    if let Some(next_elem) = next_node.get_next_element_sibling() {
                        if next_elem.get_name().to_uppercase() == "BR" {
                            break;
                        }
                    }
                }

                if !Self::is_phrasing_content(&next_node) {
                    break;
                }

                // Otherwise, make this node a child of the new <p>.
                let sibling = next_node.get_next_sibling();
                next_node.unlink();
                _ = p.add_child(&mut next_node);

                next = sibling;
            }

            if p.get_child_elements().is_empty() && p.get_content().trim().is_empty() {
                p.unlink();
                continue;
            }

            while let Some(mut last_child) = p.get_last_child() {
                let is_text_node = last_child
                    .get_type()
                    .map(|t| t == NodeType::TextNode)
                    .unwrap_or(false);
                let is_empty = last_child.get_content().trim().is_empty();

                if is_text_node && is_empty {
                    last_child.unlink();
                } else {
                    break;
                }
            }

            if let Some(mut parent) = p.get_parent() {
                if parent.get_name().to_uppercase() == "P" {
                    _ = parent.set_name("DIV");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use libxml::parser::Parser;

    use super::Util;

    fn replace_brs(source: &str, expected: &str) {
        libxml::tree::node::set_node_rc_guard(10);

        let parser = Parser::default_html();
        let document = parser.parse_string(source).unwrap();
        let root = document.get_root_element().unwrap();
        let body = root.get_first_child().unwrap();
        let div = body.get_first_child().unwrap();

        Util::replace_brs(&root, &document);

        let result = document.node_to_string(&div);

        assert_eq!(expected, result);
    }

    #[test]
    fn replace_brs_1() {
        replace_brs(
            "<div>foo<br>bar<br> <br><br>abc</div>",
            "<div>foo<br/>bar<p>abc</p></div>",
        )
    }

    #[test]
    fn replace_brs_2() {
        let source = r#"
        <div>
            <p>
                It might have been curiosity or it might have been the nagging sensation that chewed at his brain for the three weeks that he researched the subject of the conversation. All For One was a cryptid. Mystical in more ways than one, he was only a rumour on a network that was two-hundred years old. There were whispers of a shadowy figure who once ruled Japan, intermingled with a string of conspiracies and fragmented events.
            </p>
            <p>
                Izuku had even braved the dark web, poking and prodding at some of the seedier elements of the world wide web. The internet had rumours, but the dark web had stories.<br/>
            </p>
            <p>
                An implied yakuza wrote about his grandfather who lost a fire manipulation Quirk and his sanity without any reason. His grandfather had been institutionalised, crying and repeating “he took it, he took it” until his dying days. No one could console him.
            </p>
        </div>
        "#;
        replace_brs(source, source.trim())
    }
}

#[derive(Debug, Clone)]
pub struct ImageObject {
    width: Option<u32>,
    height: Option<u32>,
    url: Option<Url>,
    description: Option<String>,
    name: Option<String>,
}

impl ImageObject {
    pub fn parse_node(node: &Node) -> Option<Self> {
        if node.get_name().to_uppercase() != "DIV" {
            return None;
        }

        let item_prop_image = node
            .get_attribute("itemprop")
            .map(|prop| prop == "image")
            .unwrap_or(false);
        let item_type_image = node
            .get_attribute("itemtype")
            .map(|attr| attr == "https://schema.org/ImageObject")
            .unwrap_or(false);

        if !item_prop_image && !item_type_image {
            return None;
        }

        let meta_nodes = Util::get_elements_by_tag_name(node, "meta");

        let mut width = None;
        let mut height = None;
        let mut url = None;
        let mut description = None;
        let mut name = None;

        for meta_node in meta_nodes {
            let item_prop = meta_node.get_attribute("itemprop");
            let content_prop = meta_node.get_attribute("content");

            if let (Some(item_prop), Some(content_prop)) = (item_prop, content_prop) {
                if item_prop == "width" {
                    width = content_prop.parse::<u32>().ok();
                } else if item_prop == "height" {
                    height = content_prop.parse::<u32>().ok();
                } else if item_prop == "url" {
                    url = Url::parse(&content_prop).ok();
                } else if item_prop == "description" {
                    description = Some(content_prop);
                } else if item_prop == "name" {
                    name = Some(content_prop);
                }
            }
        }

        url.as_ref()?;

        Some(Self {
            width,
            height,
            url,
            description,
            name,
        })
    }

    pub fn replace(&self, node: &mut Node) -> Result<(), FullTextParserError> {
        let mut parent = node.get_parent().ok_or(FullTextParserError::Xml)?;

        if parent.get_name().to_uppercase() == "A" {
            return self.replace(&mut parent);
        }

        node.unlink();

        let mut root = parent
            .new_child(None, "imageobject")
            .map_err(|_| FullTextParserError::Xml)?;

        let mut a = root
            .new_child(None, "a")
            .map_err(|_| FullTextParserError::Xml)?;

        let mut img = a
            .new_child(None, "img")
            .map_err(|_| FullTextParserError::Xml)?;

        if let Some(width) = self.width {
            _ = img.set_attribute("width", &width.to_string());
        }

        if let Some(height) = self.height {
            _ = img.set_attribute("height", &height.to_string());
        }

        if let Some(description) = self.description.as_deref() {
            _ = img.set_attribute("alt", description);
        }

        if let Some(name) = self.name.as_deref() {
            _ = img.set_attribute("title", name);
        }

        if let Some(url) = self.url.as_ref() {
            _ = a.set_attribute("href", url.as_str());
            _ = img.set_attribute("src", url.as_str());
        }

        Ok(())
    }
}

pub struct VideoObject {
    thumbnail_url: Option<String>,
    content_url: Option<Url>,
    embed_url: Option<Url>,
    description: Option<String>,
    name: Option<String>,
}

impl VideoObject {
    pub fn parse_node(node: &Node) -> Option<Self> {
        if node.get_name().to_uppercase() != "DIV" {
            return None;
        }

        let item_prop_video = node
            .get_attribute("itemprop")
            .map(|prop| prop == "video")
            .unwrap_or(false);
        let item_type_video = node
            .get_attribute("itemtype")
            .map(|attr| attr == "http://schema.org/VideoObject")
            .unwrap_or(false);

        if !item_prop_video && !item_type_video {
            return None;
        }

        let meta_nodes = Util::get_elements_by_tag_name(node, "meta");

        let mut thumbnail_url = None;
        let mut content_url = None;
        let mut embed_url = None;
        let mut description = None;
        let mut name = None;

        for meta_node in meta_nodes {
            let item_prop = meta_node.get_attribute("itemprop");
            let content_prop = meta_node.get_attribute("content");

            if let (Some(item_prop), Some(content_prop)) = (item_prop, content_prop) {
                if item_prop == "thumbnailUrl" {
                    thumbnail_url = Some(content_prop);
                } else if item_prop == "contentURL" {
                    content_url = Self::parse_url(&content_prop);
                } else if item_prop == "embedURL" {
                    embed_url = Self::parse_url(&content_prop);
                } else if item_prop == "description" {
                    description = Some(content_prop);
                } else if item_prop == "name" {
                    name = Some(content_prop);
                }
            }
        }

        if thumbnail_url.is_none()
            && content_url.is_none()
            && embed_url.is_none()
            && description.is_none()
            && name.is_none()
        {
            return None;
        }

        Some(Self {
            thumbnail_url,
            content_url,
            embed_url,
            description,
            name,
        })
    }

    fn parse_url(url: &str) -> Option<Url> {
        let url = if url.starts_with("//") {
            format!("https:{url}")
        } else {
            url.into()
        };

        Url::parse(&url)
            .map_err(|err| log::error!("parse video object url: {err}"))
            .ok()
    }

    pub fn replace(&self, node: &mut Node) -> Result<(), FullTextParserError> {
        let mut parent = node.get_parent().ok_or(FullTextParserError::Xml)?;
        node.unlink();

        let mut root = parent
            .new_child(None, "videoobject")
            .map_err(|_| FullTextParserError::Xml)?;

        if let Some(name) = self.name.as_deref() {
            let mut title = root
                .new_child(None, "h3")
                .map_err(|_| FullTextParserError::Xml)?;
            _ = title.set_content(name);
        }

        if self.name != self.description {
            if let Some(description) = self.description.as_deref() {
                let mut desc = root
                    .new_child(None, "p")
                    .map_err(|_| FullTextParserError::Xml)?;
                _ = desc.set_content(description);
            }
        }

        let mut a = root
            .new_child(None, "a")
            .map_err(|_| FullTextParserError::Xml)?;
        if let Some(embed_url) = self.embed_url.as_ref() {
            _ = a.set_attribute("href", embed_url.as_str());
        } else if let Some(content_url) = self.content_url.as_ref() {
            _ = a.set_attribute("href", content_url.as_str());
        }

        let mut img = a
            .new_child(None, "img")
            .map_err(|_| FullTextParserError::Xml)?;
        if let Some(thumbnail_url) = self.thumbnail_url.as_deref() {
            _ = img.set_attribute("src", thumbnail_url);
        }

        Ok(())
    }
}
