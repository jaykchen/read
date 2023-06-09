pub mod constants;
pub mod macros;
pub mod util;

use constants::{
    ALTER_TO_DIV_EXCEPTIONS, BASE64_DATA_URL, BYLINE, COPY_TO_SRC, COPY_TO_SRCSET, DATA_TABLE_ATTR,
    DEFAULT_CHAR_THRESHOLD, DEFAULT_TAGS_TO_SCORE, DEPRECATED_SIZE_ATTRIBUTE_ELEMS, IS_BASE64,
    IS_IMAGE, MINIMUM_TOPCANDIDATES, OKAY_MAYBE_ITS_A_CANDIDATE, PRESENTATIONAL_ATTRIBUTES,
    SCORE_ATTR, SHARE_ELEMENTS, SIBLING_CONTENT, SRC_SET_URL, TITLE_CUT_END, TITLE_CUT_FRONT,
    TITLE_SEPARATOR, UNLIELY_CANDIDATES, UNLIKELY_ROLES, VALID_EMPTY_TAGS, WORD_COUNT,
};

use chrono::{DateTime, Utc};
use libxml::{
    parser::Parser,
    tree::{Document, Node, NodeType},
    xpath::Context,
};
use util::Util;

use std::cmp::Ordering;
use std::collections::HashSet;
use thiserror::Error;

use url::Url;
pub struct State {
    pub strip_unlikely: bool,
    pub weigh_classes: bool,
    pub clean_conditionally: bool,
    pub should_remove_title_header: bool,
    pub byline: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            strip_unlikely: true,
            weigh_classes: true,
            clean_conditionally: true,
            should_remove_title_header: true,
            byline: None,
        }
    }
}

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
pub struct Article {
    pub title: Option<String>,
    pub author: Option<String>,
    pub url: Url,
    pub date: Option<DateTime<Utc>>,
    pub thumbnail_url: Option<String>,
    pub document: Option<Document>,
    pub root_node: Option<Node>,
}

pub struct Readability;

impl Readability {
    pub async fn extract(
        html: &str,
        base_url: Option<url::Url>,
    ) -> Result<String, FullTextParserError> {
        libxml::tree::node::set_node_rc_guard(10);
        let empty_config = ConfigEntry::default();

        let url = base_url.unwrap_or_else(|| Url::parse("http://fakehost/test/base/").unwrap());
        let document = parse_html(html, None, &empty_config)?;
        let xpath_ctx = get_xpath_ctx(&document)?;

        prep_content(&xpath_ctx, None, &empty_config, &url, &document, None);
        let mut article = Article {
            title: None,
            author: None,
            url,
            date: None,
            thumbnail_url: None,
            document: None,
            root_node: None,
        };

        let mut article_document = Document::new().map_err(|()| FullTextParserError::Xml)?;
        let mut root =
            Node::new("article", None, &document).map_err(|()| FullTextParserError::Xml)?;
        article_document.set_root_element(&root);

        meta_extract(&xpath_ctx, None, None, &mut article);
        Readability::extract_body(document, &mut root, article.title.as_deref())?;
        post_process_document(&article_document)?;

        article.document = Some(article_document);
        article.root_node = Some(root);
        let html = article
            .get_content()
            .ok_or(FullTextParserError::Readability)?;

        Ok(html)
    }

    pub fn extract_body(
        document: Document,
        root: &mut Node,
        title: Option<&str>,
    ) -> Result<bool, FullTextParserError> {
        let mut state = State::default();
        let mut document = document;
        let mut attempts: Vec<(Node, usize, Document)> = Vec::new();
        let document_cache = document
            .dup()
            .map_err(|()| FullTextParserError::Readability)?;

        loop {
            let mut elements_to_score = Vec::new();
            let mut node: Option<Node> = document.clone().get_root_element();

            while let Some(node_ref) = node.as_mut() {
                let tag_name = node_ref.get_name().to_uppercase();

                if tag_name == "TEXT" && node_ref.get_content().trim().is_empty() {
                    node = Util::next_node(node_ref, true);
                    continue;
                }

                let match_string = node_ref
                    .get_class_names()
                    .iter()
                    .fold(String::new(), |a, b| format!("{a} {b}"));
                let match_string = match node_ref.get_property("id") {
                    Some(id) => format!("{match_string} {id}"),
                    None => match_string,
                };

                if !Util::is_probably_visible(node_ref) {
                    node = Util::remove_and_next(node_ref);
                    continue;
                }

                if Self::check_byline(node_ref, &match_string, &mut state) {
                    node = Util::remove_and_next(node_ref);
                    continue;
                }

                if state.should_remove_title_header
                    && Util::header_duplicates_title(node_ref, title)
                {
                    state.should_remove_title_header = false;
                    node = Util::remove_and_next(node_ref);
                    continue;
                }

                // Remove unlikely candidates
                if state.strip_unlikely {
                    if UNLIELY_CANDIDATES.is_match(&match_string)
                        && !OKAY_MAYBE_ITS_A_CANDIDATE.is_match(&match_string)
                        && !Util::has_ancestor_tag(
                            node_ref,
                            "table",
                            None,
                            None::<fn(&Node) -> bool>,
                        )
                        && !Util::has_ancestor_tag(
                            node_ref,
                            "code",
                            None,
                            None::<fn(&Node) -> bool>,
                        )
                        && tag_name != "BODY"
                        && tag_name != "A"
                    {
                        node = Util::remove_and_next(node_ref);
                        continue;
                    }

                    if let Some(role) = node_ref.get_attribute("role") {
                        if UNLIKELY_ROLES.contains(&role.as_str()) {
                            node = Util::remove_and_next(node_ref);
                            continue;
                        }
                    }
                }

                // Remove DIV, SECTION, and HEADER nodes without any content(e.g. text, image, video, or iframe).
                if (tag_name == "DIV"
                    || tag_name == "SECTION"
                    || tag_name == "HEADER"
                    || tag_name == "H1"
                    || tag_name == "H2"
                    || tag_name == "H3"
                    || tag_name == "H4"
                    || tag_name == "H5"
                    || tag_name == "H6")
                    && Util::is_element_without_content(node_ref)
                {
                    node = Util::remove_and_next(node_ref);
                    continue;
                }

                if DEFAULT_TAGS_TO_SCORE.contains(&tag_name.as_str()) {
                    elements_to_score.push(node_ref.clone());
                }

                // Turn all divs that don't have children block level elements into p's
                if tag_name == "DIV" {
                    // Put phrasing content into paragraphs.
                    let mut p: Option<Node> = None;
                    for mut child in node_ref.get_child_nodes().into_iter() {
                        if Util::is_phrasing_content(&child) {
                            if let Some(p) = p.as_mut() {
                                child.unlink();
                                p.add_child(&mut child).map_err(|error| {
                                    log::error!("{error}");
                                    FullTextParserError::Readability
                                })?;
                            } else if !Util::is_whitespace(&child) {
                                let mut new_node = Node::new("p", None, &document)
                                    .map_err(|()| FullTextParserError::Readability)?;
                                let mut old_node = node_ref
                                    .replace_child_node(new_node.clone(), child)
                                    .map_err(|error| {
                                        log::error!("{error}");
                                        FullTextParserError::Readability
                                    })?;

                                new_node.add_child(&mut old_node).map_err(|error| {
                                    log::error!("{error}");
                                    FullTextParserError::Readability
                                })?;
                                p.replace(new_node);
                            }
                        } else if p.is_some() {
                            if let Some(p) = p.as_mut() {
                                for mut r_node in p.get_child_nodes().into_iter().rev() {
                                    if Util::is_whitespace(&r_node) {
                                        r_node.unlink();
                                        continue;
                                    }
                                    break;
                                }
                            }
                            _ = p.take();
                        }
                    }

                    // Sites like http://mobile.slate.com encloses each paragraph with a DIV
                    // element. DIVs with only a P element inside and no text content can be
                    // safely converted into plain P elements to avoid confusing the scoring
                    // algorithm with DIVs with are, in practice, paragraphs.
                    if Util::has_single_tag_inside_element(node_ref, "P")
                        && Util::get_link_density(node_ref) < 0.25
                    {
                        if let Some(new_node) = node_ref.get_first_element_child() {
                            if let Some(mut parent) = node_ref.get_parent() {
                                parent
                                    .replace_child_node(new_node.clone(), node_ref.clone())
                                    .map_err(|error| {
                                        log::error!("{error}");
                                        FullTextParserError::Readability
                                    })?;
                                node = Util::next_node(&new_node, false);
                                elements_to_score.push(new_node.clone());
                                continue;
                            }
                        }
                    } else if !Util::has_child_block_element(node_ref)
                        && node_ref.set_name("P").is_ok()
                    {
                        elements_to_score.push(node_ref.clone());
                    }
                }

                node = Util::next_node(node_ref, false);
            }

            let mut candidates = Vec::new();
            // Loop through all paragraphs, and assign a score to them based on how content-y they look.
            // Then add their score to their parent node.
            // A score is determined by things like number of commas, class names, etc. Maybe eventually link density.
            for element_to_score in elements_to_score.drain(..) {
                if element_to_score.get_parent().is_none() {
                    continue;
                }

                let inner_text = Util::get_inner_text(&element_to_score, true);
                let inner_text_len = inner_text.len();

                // If this paragraph is less than 25 characters, don't even count it.
                if inner_text_len < 25 {
                    continue;
                }

                // Exclude nodes with no ancestor.
                let ancestors = Util::get_node_ancestors(&element_to_score, Some(5));
                if ancestors.is_empty() {
                    continue;
                }

                let mut content_score = 0.0;

                // Add a point for the paragraph itself as a base.
                content_score += 1.0;

                // Add points for any commas within this paragraph.
                content_score += inner_text.split(',').count() as f64;

                // For every 100 characters in this paragraph, add another point. Up to 3 points.
                content_score += f64::min(f64::floor(inner_text.len() as f64 / 100.0), 3.0);

                // Initialize and score ancestors.
                for (level, mut ancestor) in ancestors.into_iter().enumerate() {
                    let tag_name = ancestor.get_name().to_uppercase();

                    if ancestor.get_parent().is_none() || tag_name == "HTML" {
                        continue;
                    }

                    if Self::get_content_score(&ancestor).is_none() {
                        Self::initialize_node(&mut ancestor, &state)?;
                        candidates.push(ancestor.clone());
                    }

                    // Node score divider:
                    // - parent:             1 (no division)
                    // - grandparent:        2
                    // - great grandparent+: ancestor level * 3
                    let score_divider = if level == 0 {
                        1.0
                    } else if level == 1 {
                        2.0
                    } else {
                        level as f64 * 3.0
                    };

                    if let Some(score) = Self::get_content_score(&ancestor) {
                        let add_score = content_score / score_divider;
                        let new_score = score + add_score;

                        Self::set_content_score(&mut ancestor, new_score)?;
                    }
                }
            }

            candidates.sort_by(|a, b| {
                if let (Some(a), Some(b)) = (Self::get_content_score(a), Self::get_content_score(b))
                {
                    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
                } else {
                    Ordering::Equal
                }
            });

            let top_candidates = candidates.into_iter().take(5).collect::<Vec<_>>();

            let mut needed_to_create_top_candidate = false;
            let mut top_candidate = top_candidates.first().cloned().unwrap_or_else(|| {
                // If we still have no top candidate, just use the body as a last resort.
                // We also have to copy the body node so it is something we can modify.
                let mut root = document.get_root_element().expect("doc should have root");
                if let Some(body) = root
                    .get_child_elements()
                    .into_iter()
                    .find(|n| n.get_name().to_uppercase() == "BODY")
                {
                    root = body;
                }

                let mut new_top_candidate =
                    Node::new("DIV", None, &document).expect("can't create new node");

                for mut child in root.get_child_elements().drain(..) {
                    child.unlink();
                    new_top_candidate.add_child(&mut child).unwrap();
                }

                root.add_child(&mut new_top_candidate).unwrap();

                Self::initialize_node(&mut new_top_candidate, &state)
                    .expect("init should not fail");
                needed_to_create_top_candidate = true;
                new_top_candidate
            });

            // Util::serialize_node(&top_candidate, "top_candidate.html");

            let mut alternative_candidate_ancestors = Vec::new();
            // Find a better top candidate node if it contains (at least three) nodes which belong to `topCandidates` array
            // and whose scores are quite closed with current `topCandidate` node.
            if let Some(top_score) = Self::get_content_score(&top_candidate) {
                for candidate in top_candidates.iter().skip(1) {
                    let score = Self::get_content_score(candidate).unwrap_or(0.0);
                    if score / top_score >= 0.75 {
                        alternative_candidate_ancestors
                            .push(Util::get_node_ancestors(candidate, None));
                    }
                }
            }

            if alternative_candidate_ancestors.len() >= MINIMUM_TOPCANDIDATES {
                let mut parent_of_top_candidate = top_candidate.get_parent();

                while let Some(parent) = &parent_of_top_candidate {
                    if parent.get_name().to_uppercase() == "BODY" {
                        break;
                    }

                    let mut lists_containing_this_ancestor = 0;
                    let tmp =
                        usize::min(alternative_candidate_ancestors.len(), MINIMUM_TOPCANDIDATES);
                    for ancestors in alternative_candidate_ancestors.iter().take(tmp) {
                        lists_containing_this_ancestor +=
                            ancestors.iter().filter(|n| n == &parent).count();
                    }

                    if lists_containing_this_ancestor >= MINIMUM_TOPCANDIDATES {
                        top_candidate = parent.clone();
                        break;
                    }

                    parent_of_top_candidate = parent_of_top_candidate.and_then(|n| n.get_parent());
                }
            }

            if Self::get_content_score(&top_candidate).is_none() {
                Self::initialize_node(&mut top_candidate, &state)?;
            }

            let mut parent_of_top_candidate = top_candidate.get_parent();
            let mut last_score = Self::get_content_score(&top_candidate).unwrap_or(0.0);

            // The scores shouldn't get too low.
            let score_threshold = last_score / 3.0;

            while parent_of_top_candidate.is_some()
                && !Util::has_tag_name(parent_of_top_candidate.as_ref(), "BODY")
            {
                if parent_of_top_candidate
                    .as_ref()
                    .map(|n| Self::get_content_score(n).is_none())
                    .unwrap_or(false)
                {
                    parent_of_top_candidate = parent_of_top_candidate.and_then(|n| n.get_parent());
                    continue;
                }

                let parent_score = parent_of_top_candidate
                    .as_ref()
                    .and_then(Self::get_content_score)
                    .unwrap_or(0.0);
                if parent_score < score_threshold {
                    break;
                }

                if parent_score > last_score {
                    // Alright! We found a better parent to use.
                    if let Some(parent) = parent_of_top_candidate {
                        top_candidate = parent;
                    }
                    break;
                }

                last_score = parent_of_top_candidate
                    .as_ref()
                    .and_then(Self::get_content_score)
                    .unwrap_or(0.0);
                parent_of_top_candidate = parent_of_top_candidate.and_then(|n| n.get_parent());
            }

            // If the top candidate is the only child, use parent instead. This will help sibling
            // joining logic when adjacent content is actually located in parent's sibling node.
            parent_of_top_candidate = top_candidate.get_parent();

            while !Util::has_tag_name(parent_of_top_candidate.as_ref(), "BODY")
                && parent_of_top_candidate
                    .as_ref()
                    .map(|n| n.get_child_elements().len() == 1)
                    .unwrap_or(false)
            {
                top_candidate = parent_of_top_candidate.ok_or(FullTextParserError::Readability)?;
                parent_of_top_candidate = top_candidate.get_parent();
            }

            if Self::get_content_score(&top_candidate).is_none() {
                Self::initialize_node(&mut top_candidate, &state)?;
            }

            // Now that we have the top candidate, look through its siblings for content
            // that might also be related. Things like preambles, content split by ads
            // that we removed, etc.
            let mut article_content =
                Node::new("DIV", None, &document).map_err(|()| FullTextParserError::Readability)?;

            let sibling_score_threshold = f64::max(
                8.0, //lowered from 10.0
                Self::get_content_score(&top_candidate).unwrap_or(0.0) * 0.2,
            );
            // Keep potential top candidate's parent node to try to get text direction of it later.
            parent_of_top_candidate = top_candidate.get_parent();
            let siblings = parent_of_top_candidate
                .as_ref()
                .map(|n| n.get_child_elements());

            if let Some(mut siblings) = siblings {
                for mut sibling in siblings.drain(..) {
                    let mut append = false;

                    let score = Self::get_content_score(&sibling).unwrap_or(0.0);

                    if top_candidate == sibling {
                        append = true;
                    } else {
                        let mut content_bonus = 0.0;

                        // Give a bonus if sibling nodes and top candidates have the example same classname
                        let sibling_classes = sibling.get_class_names();
                        let tc_classes = top_candidate.get_class_names();

                        if !tc_classes.is_empty()
                            && !sibling_classes.is_empty()
                            && sibling_classes
                                .iter()
                                .all(|class| tc_classes.contains(class))
                        {
                            content_bonus +=
                                Self::get_content_score(&top_candidate).unwrap_or(0.0) * 0.2;
                        }

                        if score + content_bonus >= sibling_score_threshold {
                            append = true;
                        } else if sibling.get_name().to_uppercase() == "P" {
                            let link_density = Util::get_link_density(&sibling);
                            let node_content = Util::get_inner_text(&sibling, true);
                            let node_length = node_content.len();

                            if node_length > 80
                                && (link_density < 0.25
                                    || (node_length > 0
                                        && link_density == 0.0
                                        && SIBLING_CONTENT.is_match(&node_content)))
                            {
                                append = true;
                            }
                        }
                    }

                    if append {
                        log::debug!(
                            "Appending node: {} ({:?})",
                            sibling.get_name(),
                            sibling.get_attribute("class")
                        );

                        if !ALTER_TO_DIV_EXCEPTIONS
                            .contains(sibling.get_name().to_uppercase().as_str())
                        {
                            // We have a node that isn't a common block level element, like a form or td tag.
                            // Turn it into a div so it doesn't get filtered out later by accident.
                            log::debug!(
                                "Altering sibling: {} ({:?})",
                                sibling.get_name(),
                                sibling.get_attribute("class")
                            );

                            sibling.set_name("DIV").map_err(|error| {
                                log::error!("{error}");
                                FullTextParserError::Readability
                            })?;
                        }

                        sibling.unlink();
                        article_content.add_child(&mut sibling).map_err(|error| {
                            log::error!("{error}");
                            FullTextParserError::Readability
                        })?;
                    }
                }
            }

            if state.clean_conditionally {
                post_process_page(&mut article_content)?;
            }

            if needed_to_create_top_candidate {
                // We already created a fake div thing, and there wouldn't have been any siblings left
                // for the previous loop, so there's no point trying to create a new div, and then
                // move all the children over. Just assign IDs and class names here. No need to append
                // because that already happened anyway.
                top_candidate
                    .set_property("id", "readability-page-1")
                    .map_err(|error| {
                        log::error!("{error}");
                        FullTextParserError::Readability
                    })?;
            } else {
                let mut div = Node::new("DIV", None, &document)
                    .map_err(|()| FullTextParserError::Readability)?;
                div.set_property("id", "readability-page-1")
                    .map_err(|error| {
                        log::error!("{error}");
                        FullTextParserError::Readability
                    })?;

                for mut child in article_content.get_child_nodes() {
                    child.unlink();
                    div.add_child(&mut child).map_err(|error| {
                        log::error!("{error}");
                        FullTextParserError::Readability
                    })?;
                }
                article_content.add_child(&mut div).map_err(|error| {
                    log::error!("{error}");
                    FullTextParserError::Readability
                })?;
            }

            let mut parse_successful = true;

            // Now that we've gone through the full algorithm, check to see if
            // we got any meaningful content. If we didn't, we may need to re-run
            // grabArticle with different flags set. This gives us a higher likelihood of
            // finding the content, and the sieve approach gives us a higher likelihood of
            // finding the -right- content.
            let text = Util::get_inner_text(&article_content, true);
            let text_length = text.len();

            if text_length < DEFAULT_CHAR_THRESHOLD {
                parse_successful = false;

                if state.strip_unlikely {
                    state.strip_unlikely = false;
                    attempts.push((article_content, text_length, document));
                } else if state.weigh_classes {
                    state.weigh_classes = false;
                    attempts.push((article_content, text_length, document));
                } else if state.clean_conditionally {
                    state.clean_conditionally = false;
                    attempts.push((article_content, text_length, document));
                } else {
                    attempts.push((article_content, text_length, document));
                    // No luck after removing flags, just return the longest text we found during the different loops

                    attempts.sort_by(|(_, size_a, _), (_, size_b, _)| size_a.cmp(size_b));

                    // But first check if we actually have something
                    if let Some((best_attempt, _len, _document)) = attempts.pop() {
                        for mut child in best_attempt.get_child_nodes() {
                            child.unlink();
                            root.add_child(&mut child).map_err(|error| {
                                log::error!("{error}");
                                FullTextParserError::Readability
                            })?;
                        }
                        parse_successful = true;
                    }

                    return Ok(parse_successful);
                }

                document = document_cache
                    .dup()
                    .map_err(|()| FullTextParserError::Readability)?;
            } else {
                for mut child in article_content.get_child_nodes() {
                    child.unlink();
                    root.add_child(&mut child).map_err(|error| {
                        log::error!("{error}");
                        FullTextParserError::Readability
                    })?;
                }
                return Ok(parse_successful);
            }
        }
    }

    fn get_content_score(node: &Node) -> Option<f64> {
        node.get_attribute(SCORE_ATTR)
            .and_then(|a| a.parse::<f64>().ok())
    }

    fn set_content_score(node: &mut Node, score: f64) -> Result<(), FullTextParserError> {
        node.set_attribute(SCORE_ATTR, &score.to_string())
            .map_err(|err| {
                log::error!("failed to set content score: {err}");
                FullTextParserError::Readability
            })
    }

    fn check_byline(node: &Node, matchstring: &str, state: &mut State) -> bool {
        if state.byline.is_some() {
            return false;
        }

        let rel = node
            .get_attribute("rel")
            .map(|rel| rel == "author")
            .unwrap_or(false);
        let itemprop = node
            .get_attribute("itemprop")
            .map(|prop| prop.contains("author"))
            .unwrap_or(false);

        let content = node.get_content();
        if rel || itemprop || BYLINE.is_match(matchstring) && Self::is_valid_byline(&content) {
            state.byline = Some(content.trim().into());
            true
        } else {
            false
        }
    }

    // Check whether the input string could be a byline.
    // This verifies that the input length is less than 100 chars.
    fn is_valid_byline(line: &str) -> bool {
        let len = line.trim().len();
        len > 0 && len < 100
    }

    // Initialize a node with the readability object. Also checks the
    // className/id for special names to add to its score.
    fn initialize_node(node: &mut Node, state: &State) -> Result<(), FullTextParserError> {
        let score = match node.get_name().to_uppercase().as_str() {
            "DIV" => 5,
            "PRE" | "TD" | "BLOCKQUITE" => 3,
            "DL" | "DD" | "DT" => -3,
            // "ADDRESS" | "OL" | "UL" | "DL" | "DD" | "DT" | "LI" | "FORM" => -3,
            "H1" | "H2" | "H3" | "H4" | "H5" | "H6" | "TH" => -2, // increased from -5
            _ => 0,
        };
        let class_weight = if state.weigh_classes {
            Util::get_class_weight(node)
        } else {
            0
        };
        let score = score + class_weight;
        log::debug!(
            "initialize node {} {}: {score}",
            node.get_name(),
            node.get_attribute("class").unwrap_or_default()
        );
        Self::set_content_score(node, score as f64)?;
        Ok(())
    }
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
macro_rules! extract_vec_multi {
    (
            $line: ident,
            $identifier: ident,
            $vector: ident
        ) => {
        if $line.starts_with($identifier) {
            let value = Util::str_extract_value($identifier, $line);
            let value = Util::split_values(value);
            let value: Vec<String> = value.iter().map(|s| s.trim().to_string()).collect();
            $vector.extend(value);
            continue;
        }
    };
}

macro_rules! extract_vec_single {
    (
            $line: ident,
            $identifier: ident,
            $vector: ident
        ) => {
        if $line.starts_with($identifier) {
            let value = Util::str_extract_value($identifier, $line);
            $vector.push(value.to_string());
            continue;
        }
    };
}

macro_rules! extract_option_single {
    (
            $line: ident,
            $identifier: ident,
            $option: ident
        ) => {
        if $line.starts_with($identifier) {
            let value = Util::str_extract_value($identifier, $line);
            $option = Some(value.to_string());
            continue;
        }
    };
}

pub fn post_process_document(document: &Document) -> Result<(), FullTextParserError> {
    if let Some(mut root) = document.get_root_element() {
        simplify_nested_elements(&mut root)?;
        clean_attributes(&mut root)?;
        remove_single_cell_tables(&mut root);
        remove_extra_p_and_div(&mut root);
    }

    Ok(())
}

pub fn meta_extract(
    context: &Context,
    config: Option<&ConfigEntry>,
    global_config: Option<&ConfigEntry>,
    article: &mut Article,
) {
    if article.title.is_none() {
        article.title = extract_title(context, config, global_config)
            .map(|title| match escaper::decode_html(&title) {
                Ok(escaped_title) => escaped_title,
                Err(_error) => title,
            })
            .map(|title| {
                // clean titles that contain separators
                if TITLE_SEPARATOR.is_match(&title) {
                    let new_title = TITLE_CUT_END.replace(&title, "$1");
                    let word_count = WORD_COUNT.split(&title).count();
                    if word_count < 3 {
                        TITLE_CUT_FRONT.replace(&title, "$1").trim().to_string()
                    } else {
                        new_title.trim().to_string()
                    }
                } else {
                    title
                }
            });
    }
}

pub fn parse_html(
    html: &str,
    config: Option<&ConfigEntry>,
    global_config: &ConfigEntry,
) -> Result<Document, FullTextParserError> {
    // replace matches in raw html

    let mut html = html.to_owned();
    if let Some(config) = config {
        for replace in &config.replace {
            html = html.replace(&replace.to_replace, &replace.replace_with);
        }
    }

    for replace in &global_config.replace {
        html = html.replace(&replace.to_replace, &replace.replace_with);
    }

    // parse html
    let parser = Parser::default_html();
    parser.parse_string(html.as_str()).map_err(|err| {
        log::error!("Parsing HTML failed for downloaded HTML {:?}", err);
        FullTextParserError::Xml
    })
}

pub fn get_xpath_ctx(doc: &Document) -> Result<Context, FullTextParserError> {
    Context::new(doc).map_err(|()| {
        log::error!("Creating xpath context failed for downloaded HTML");
        FullTextParserError::Xml
    })
}

pub fn prep_content(
    context: &Context,
    config: Option<&ConfigEntry>,
    global_config: &ConfigEntry,
    url: &Url,
    document: &Document,
    title: Option<&str>,
) {
    // replace H1 with H2 as H1 should be only title that is displayed separately
    if let Ok(h1_nodes) = Util::evaluate_xpath(context, "//h1", false) {
        for mut h1_node in h1_nodes {
            _ = h1_node.set_name("h2");
        }
    }

    if let Ok(h2_nodes) = Util::evaluate_xpath(context, "//h2", false) {
        for mut h2_node in h2_nodes {
            if Util::header_duplicates_title(&h2_node, title) {
                h2_node.unlink();
            }
        }
    }

    // rename all font nodes to span
    if let Ok(font_nodes) = Util::evaluate_xpath(context, "//font", false) {
        for mut font_node in font_nodes {
            _ = font_node.set_name("span");
        }
    }

    _ = Util::mark_data_tables(context);

    // strip specified xpath
    if let Some(config) = config {
        for xpath_strip in &config.xpath_strip {
            _ = Util::strip_node(context, xpath_strip);
        }
    }

    for xpath_strip in &global_config.xpath_strip {
        _ = Util::strip_node(context, xpath_strip);
    }

    // strip everything with specified 'id' or 'class'
    if let Some(config) = config {
        for xpaht_strip_class in &config.strip_id_or_class {
            _ = Util::strip_id_or_class(context, xpaht_strip_class);
        }
    }

    for xpaht_strip_class in &global_config.strip_id_or_class {
        _ = Util::strip_id_or_class(context, xpaht_strip_class);
    }

    // strip any <img> element where @src attribute contains this substring
    if let Some(config) = config {
        for xpath_strip_img_src in &config.strip_image_src {
            _ = Util::strip_node(
                context,
                &format!("//img[contains(@src,'{}')]", xpath_strip_img_src),
            );
        }
    }

    for xpath_strip_img_src in &global_config.strip_image_src {
        _ = Util::strip_node(
            context,
            &format!("//img[contains(@src,'{}')]", xpath_strip_img_src),
        );
    }

    _ = Util::strip_node(context, "//noscript");

    _ = fix_lazy_images(context, document);
    _ = fix_iframe_size(context, "youtube.com");
    _ = remove_attribute(context, Some("a"), "onclick");

    // strip elements using Readability.com and Instapaper.com ignore class names
    // .entry-unrelated and .instapaper_ignore
    // See http://blog.instapaper.com/post/730281947
    _ = Util::strip_node(
        context,
        "//*[contains(@class,' entry-unrelated ') or contains(@class,' instapaper_ignore ')]",
    );

    // strip elements that contain style="display: none;"
    _ = Util::strip_node(context, "//*[contains(@style,'display:none')]");
    _ = Util::strip_node(context, "//*[contains(@style,'display: none')]");
    _ = remove_attribute(context, None, "style");

    // strip all input elements
    _ = Util::strip_node(context, "//form");
    _ = Util::strip_node(context, "//input");
    _ = Util::strip_node(context, "//textarea");
    _ = Util::strip_node(context, "//select");
    _ = Util::strip_node(context, "//button");

    // strip all comments
    _ = Util::strip_node(context, "//comment()");

    // strip all scripts
    _ = Util::strip_node(context, "//script");

    // strip all styles
    _ = Util::strip_node(context, "//style");

    // strip all empty url-tags <a/>
    _ = Util::strip_node(context, "//a[not(node())]");

    // strip all external css and fonts
    _ = Util::strip_node(context, "//*[@type='text/css']");

    // other junk
    _ = Util::strip_node(context, "//iframe");
    _ = Util::strip_node(context, "//object");
    _ = Util::strip_node(context, "//embed");
    _ = Util::strip_node(context, "//footer");
    _ = Util::strip_node(context, "//link");
    _ = Util::strip_node(context, "//aside");

    if let Some(root) = document.get_root_element() {
        Util::replace_brs(&root, document);
    }

    fix_urls(context, url, document);
}

pub fn fix_urls(context: &Context, url: &Url, document: &Document) {
    _ = repair_urls(context, "//img", "src", url, document);
    _ = repair_urls(context, "//a", "src", url, document);
    _ = repair_urls(context, "//a", "href", url, document);
    _ = repair_urls(context, "//object", "data", url, document);
    _ = repair_urls(context, "//iframe", "src", url, document);
}

pub fn repair_urls(
    context: &Context,
    xpath: &str,
    attribute: &str,
    article_url: &url::Url,
    document: &Document,
) -> anyhow::Result<()> {
    let node_vec = Util::evaluate_xpath(context, xpath, false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;
    for mut node in node_vec {
        if let Some(url) = node.get_attribute(attribute) {
            let trimmed_url = url.trim();

            let is_hash_url = url.starts_with('#');
            let is_relative_url = url::Url::parse(&url)
                .err()
                .map(|err| err == url::ParseError::RelativeUrlWithoutBase)
                .unwrap_or(false);
            let is_javascript = trimmed_url.contains("javascript:");

            if !is_hash_url && node.get_name().to_uppercase() == "A" {
                _ = node.set_attribute("target", "_blank");
            }

            if let Some(srcset) = node.get_attribute("srcset") {
                let res = SRC_SET_URL
                    .captures_iter(&srcset)
                    .map(|cap| {
                        let cap0 = cap.get(0).map_or("", |m| m.as_str());
                        let cap1 = cap.get(1).map_or("", |m| m.as_str());
                        let cap2 = cap.get(2).map_or("", |m| m.as_str());
                        let cap3 = cap.get(3).map_or("", |m| m.as_str());

                        let is_relative_url = url::Url::parse(cap1)
                            .err()
                            .map(|err| err == url::ParseError::RelativeUrlWithoutBase)
                            .unwrap_or(false);

                        if is_relative_url {
                            let completed_url = article_url
                                .join(cap1)
                                .map(|u| u.as_str().to_owned())
                                .unwrap_or_default();
                            format!("{completed_url}{cap2}{cap3}")
                        } else {
                            cap0.to_string()
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(" ");

                _ = node.set_attribute("srcset", res.as_str());
            }

            if is_hash_url {
                _ = node.set_attribute(attribute, trimmed_url);
            } else if is_relative_url {
                let completed_url = match article_url.join(trimmed_url) {
                    Ok(joined_url) => joined_url,
                    Err(_) => continue,
                };
                _ = node.set_attribute(attribute, completed_url.as_str());
            } else if is_javascript {
                // if the link only contains simple text content, it can be converted to a text node
                let mut child_nodes = node.get_child_nodes();
                let child_count = child_nodes.len();
                let first_child_is_text = child_nodes
                    .first()
                    .and_then(|n| n.get_type())
                    .map(|t| t == NodeType::TextNode)
                    .unwrap_or(false);
                if let Some(mut parent) = node.get_parent() {
                    let new_node = if child_count == 1 && first_child_is_text {
                        let link_content = node.get_content();
                        Node::new_text(&link_content, document)
                            .expect("Failed to create new text node")
                    } else {
                        let mut container = Node::new("span", None, document)
                            .expect("Failed to create new span container node");
                        for mut child in child_nodes.drain(..) {
                            child.unlink();
                            _ = container.add_child(&mut child);
                        }
                        container
                    };

                    _ = parent.replace_child_node(new_node, node);
                }
            } else if let Ok(parsed_url) = Url::parse(trimmed_url) {
                _ = node.set_attribute(attribute, parsed_url.as_str());
            } else {
                _ = node.set_attribute(attribute, trimmed_url);
            };
        }
    }
    Ok(())
}

pub fn remove_attribute(
    context: &Context,
    tag: Option<&str>,
    attribute: &str,
) -> anyhow::Result<()> {
    let xpath_tag = tag.unwrap_or("*");

    let xpath = &format!("//{}[@{}]", xpath_tag, attribute);
    let node_vec = Util::evaluate_xpath(context, xpath, false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;

    Ok(())
}

pub fn fix_iframe_size(context: &Context, site_name: &str) -> anyhow::Result<()> {
    let xpath = &format!("//iframe[contains(@src, '{}')]", site_name);
    let node_vec = Util::evaluate_xpath(context, xpath, false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;
    for mut node in node_vec {
        let video_wrapper = node
            .get_parent()
            .and_then(|mut parent| parent.new_child(None, "div").ok());
        if let Some(mut video_wrapper) = video_wrapper {
            let success = video_wrapper
                .set_property("class", "videoWrapper")
                .ok()
                .and_then(|()| node.set_property("width", "100%").ok())
                .and_then(|()| node.set_property("height", "100%").ok())
                .ok_or_else(|| {
                    node.unlink();
                    video_wrapper.add_child(&mut node)
                })
                .is_err();
            if !success {
                log::warn!("Failed to add iframe as child of video wrapper <div>");
            }
        } else {
            log::warn!("Failed to get parent of iframe");
        }
    }
    Ok(())
}

pub fn fix_lazy_images(context: &Context, doc: &Document) -> anyhow::Result<()> {
    let mut img_nodes = Util::evaluate_xpath(context, "//img", false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;
    let pic_nodes = Util::evaluate_xpath(context, "//picture", false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;
    let fig_nodes = Util::evaluate_xpath(context, "//figure", false)
        .map_err(|_err| anyhow::anyhow!("Failed to evaluate XPath"))?;

    img_nodes.extend(pic_nodes);
    img_nodes.extend(fig_nodes);

    for mut node in img_nodes {
        let tag_name = node.get_name().to_uppercase();

        // In some sites (e.g. Kotaku), they put 1px square image as base64 data uri in the src attribute.
        // So, here we check if the data uri is too short, just might as well remove it.
        if let Some(src) = node.get_attribute("src") {
            // Make sure it's not SVG, because SVG can have a meaningful image in under 133 bytes.
            if let Some(mime) = BASE64_DATA_URL
                .captures(&src)
                .and_then(|c| c.get(1).map(|c| c.as_str()))
            {
                if mime == "image/svg+xml" {
                    continue;
                }
            }

            // Make sure this element has other attributes which contains image.
            // If it doesn't, then this src is important and shouldn't be removed.
            let mut src_could_be_removed = false;
            for (name, val) in node.get_attributes() {
                if name == "src" {
                    continue;
                }

                if IS_IMAGE.is_match(&val) {
                    src_could_be_removed = true;
                    break;
                }
            }

            // Here we assume if image is less than 100 bytes (or 133B after encoded to base64)
            // it will be too small, therefore it might be placeholder image.
            if src_could_be_removed {
                if let Some(_match) = IS_BASE64.find(&src) {
                    let b64starts = _match.start() + 7;
                    let b64length = src.len() - b64starts;
                    if b64length < 133 {
                        _ = node.remove_attribute("src");
                    }
                }
            }
        }

        let class_contains_lazy = node
            .get_attribute("class")
            .map(|c| c.to_lowercase().contains("lazy"))
            .unwrap_or(false);
        let has_scr = node.has_attribute("src");
        let has_srcset = node.has_attribute("srcset");

        if (has_scr || has_srcset) && !class_contains_lazy {
            continue;
        }

        for (name, val) in node.get_attributes() {
            if name == "src" || name == "srcset" || name == "alt" {
                continue;
            }

            let mut copy_to: Option<&str> = None;
            if COPY_TO_SRCSET.is_match(&val) {
                copy_to = Some("srcset");
            } else if COPY_TO_SRC.is_match(&val) {
                copy_to = Some("src");
            }

            if let Some(copy_to) = copy_to {
                //if this is an img or picture, set the attribute directly
                if tag_name == "IMG" || tag_name == "PICTURE" {
                    _ = node.set_attribute(copy_to, &val);
                } else if tag_name == "FIGURE"
                    && !Util::has_any_descendent_tag(&node, &HashSet::from(["IMG", "PICTURE"]))
                {
                    //if the item is a <figure> that does not contain an image or picture, create one and place it inside the figure
                    //see the nytimes-3 testcase for an example
                    let mut img = Node::new("img", None, doc).unwrap();
                    _ = img.set_attribute(copy_to, &val);
                    _ = node.add_child(&mut img);
                }
            }
        }
    }
    Ok(())
}

pub fn simplify_nested_elements(root: &mut Node) -> Result<(), FullTextParserError> {
    let mut node_iter = Some(root.clone());

    while let Some(mut node) = node_iter {
        let tag_name = node.get_name().to_uppercase();

        if tag_name == "ARTICLE" || node.get_parent().is_none() {
            node_iter = Util::next_node(&node, false);
            continue;
        }

        if tag_name != "DIV" && tag_name != "SECTION" {
            node_iter = Util::next_node(&node, false);
            continue;
        }

        if Util::is_element_without_content(&node) {
            node_iter = Util::remove_and_next(&mut node);
            continue;
        } else if Util::has_single_tag_inside_element(&node, "DIV")
            || Util::has_single_tag_inside_element(&node, "SECTION")
        {
            if let Some(mut parent) = node.get_parent() {
                if let Some(mut child) = node.get_child_elements().into_iter().next() {
                    for (k, v) in node.get_attributes().into_iter() {
                        child.set_attribute(&k, &v).map_err(|e| {
                            log::error!("{e}");
                            FullTextParserError::Xml
                        })?;
                    }
                    parent
                        .replace_child_node(child, node.clone())
                        .map_err(|e| {
                            log::error!("{e}");
                            FullTextParserError::Xml
                        })?;

                    node_iter = Util::next_node(&parent, false);
                    continue;
                }
            }
        }

        node_iter = Util::next_node(&node, false);
    }
    Ok(())
}

pub fn clean_attributes(root: &mut Node) -> Result<(), FullTextParserError> {
    let mut node_iter = Some(root.clone());

    while let Some(mut node) = node_iter {
        let tag_name = node.get_name().to_uppercase();

        for attr in PRESENTATIONAL_ATTRIBUTES {
            _ = node.remove_attribute(attr);
        }

        if DEPRECATED_SIZE_ATTRIBUTE_ELEMS.contains(tag_name.as_str()) {
            _ = node.remove_attribute("width");
            _ = node.remove_attribute("height");
        }

        node.remove_attribute("class").map_err(|e| {
            log::error!("{e}");
            FullTextParserError::Xml
        })?;

        node.remove_attribute("align").map_err(|e| {
            log::error!("{e}");
            FullTextParserError::Xml
        })?;

        node.remove_attribute(SCORE_ATTR).map_err(|e| {
            log::error!("{e}");
            FullTextParserError::Xml
        })?;

        node.remove_attribute(DATA_TABLE_ATTR).map_err(|e| {
            log::error!("{e}");
            FullTextParserError::Xml
        })?;

        node_iter = Util::next_node(&node, false);
    }
    Ok(())
}

pub fn remove_single_cell_tables(root: &mut Node) {
    let mut node_iter = Some(root.clone());

    while let Some(node) = node_iter {
        let tag_name = node.get_name().to_uppercase();
        if tag_name == "TABLE" {
            let t_body = if Util::has_single_tag_inside_element(&node, "TBODY") {
                node.get_child_elements().drain(..).next().unwrap()
            } else {
                node.clone()
            };
            if Util::has_single_tag_inside_element(&t_body, "TR") {
                let row = t_body.get_child_elements().first().cloned();
                if let Some(row) = row {
                    if Util::has_single_tag_inside_element(&row, "TD") {
                        let cell = row.get_child_elements().first().cloned();
                        if let Some(mut cell) = cell {
                            let all_phrasing_content = cell
                                .get_child_elements()
                                .into_iter()
                                .all(|child| Util::is_phrasing_content(&child));
                            cell.set_name(if all_phrasing_content { "P" } else { "DIV" })
                                .unwrap();
                            if let Some(mut parent) = node.get_parent() {
                                node_iter = Util::next_node(&node, true);
                                parent.replace_child_node(cell, node.clone()).unwrap();
                                continue;
                            }
                        }
                    }
                }
            }
        }

        node_iter = Util::next_node(&node, false);
    }
}

pub fn remove_extra_p_and_div(root: &mut Node) {
    let mut node_iter = Some(root.clone());

    while let Some(mut node) = node_iter {
        let tag_name = node.get_name().to_uppercase();
        if tag_name == "P" || tag_name == "DIV" {
            let img_count = Util::get_elements_by_tag_name(&node, "img").len();
            let embed_count = Util::get_elements_by_tag_name(&node, "embed").len();
            let object_count = Util::get_elements_by_tag_name(&node, "object").len();
            let iframe_count = Util::get_elements_by_tag_name(&node, "iframe").len();
            let total_count = img_count + embed_count + object_count + iframe_count;

            if total_count == 0 && Util::get_inner_text(&node, false).trim().is_empty() {
                node_iter = Util::remove_and_next(&mut node);
                continue;
            }
        }

        node_iter = Util::next_node(&node, false);
    }
}

pub fn extract_title(
    context: &Context,
    config: Option<&ConfigEntry>,
    global_config: Option<&ConfigEntry>,
) -> Option<String> {
    // check site specific config
    if let Some(config) = config {
        for xpath_title in &config.xpath_title {
            if let Ok(title) = Util::extract_value_merge(context, xpath_title) {
                return Some(title);
            }
        }
    }

    // check global config
    if let Some(global_config) = global_config {
        for xpath_title in &global_config.xpath_title {
            if let Ok(title) = Util::extract_value_merge(context, xpath_title) {
                return Some(title);
            }
        }
    }

    // generic meta (readablity)
    Util::extract_value(context, "//title")
        .ok()
        .or_else(|| get_meta(context, "dc:title"))
        .or_else(|| get_meta(context, "dcterm:title"))
        .or_else(|| get_meta(context, "og:title"))
        .or_else(|| get_meta(context, "weibo:article:title"))
        .or_else(|| get_meta(context, "weibo:webpage:title"))
        .or_else(|| get_meta(context, "twitter:title"))
}

fn get_meta(context: &Context, name: &str) -> Option<String> {
    Util::get_attribute(
        context,
        &format!("//meta[contains(@name, '{}')]", name),
        "content",
    )
    .ok()
}

pub fn post_process_page(node: &mut Node) -> Result<(), FullTextParserError> {
    Util::clean_headers(node);
    Util::replace_schema_org_orbjects(node);
    Util::clean_conditionally(node, "fieldset");
    Util::clean_conditionally(node, "table");
    Util::clean_conditionally(node, "ul");
    Util::clean_conditionally(node, "div");

    remove_share_elements(node);
    clean_attributes(node)?;
    remove_single_cell_tables(node);
    remove_extra_p_and_div(node);
    remove_empty_nodes(node);

    Ok(())
}

fn remove_empty_nodes(root: &mut Node) {
    let mut node_iter = Some(root.clone());

    while let Some(mut node) = node_iter {
        let tag_name = node.get_name().to_uppercase();

        if VALID_EMPTY_TAGS.contains(tag_name.as_str()) {
            node_iter = Util::next_node(&node, false);
            continue;
        }

        if Util::is_element_without_children(&node) {
            node_iter = Util::remove_and_next(&mut node);
            continue;
        }

        node_iter = Util::next_node(&node, false);
    }
}
fn remove_share_elements(root: &mut Node) {
    let mut node_iter = Some(root.clone());

    while let Some(mut node) = node_iter {
        let match_string = format!(
            "{} {}",
            node.get_attribute("class").unwrap_or_default(),
            node.get_attribute("id").unwrap_or_default()
        );

        if SHARE_ELEMENTS.is_match(&match_string)
            && node.get_content().len() < DEFAULT_CHAR_THRESHOLD
        {
            node_iter = Util::remove_and_next(&mut node);
        } else {
            node_iter = Util::next_node(&node, false);
        }
    }
}

impl Article {
    pub fn get_content(&self) -> Option<String> {
        if let (Some(document), Some(root)) = (self.document.as_ref(), self.root_node.as_ref()) {
            Some(document.node_to_string(root))
        } else {
            None
        }
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
