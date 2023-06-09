use std::collections::HashSet;

use once_cell::sync::Lazy;
use regex::{Regex, RegexBuilder};

pub const DEFAULT_CHAR_THRESHOLD: usize = 500;
pub static IS_IMAGE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"\.(jpg|jpeg|png|webp)"#)
        .case_insensitive(true)
        .build()
        .expect("IS_IMAGE regex")
});
pub static COPY_TO_SRCSET: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"\.(jpg|jpeg|png|webp)\s+\d"#)
        .case_insensitive(true)
        .build()
        .expect("COPY_TO_SRC regex")
});
pub static COPY_TO_SRC: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"^\s*\S+\.(jpg|jpeg|png|webp)\S*\s*$"#)
        .case_insensitive(true)
        .build()
        .expect("COPY_TO_SRC regex")
});
pub static IS_BASE64: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"base64\s*"#)
        .case_insensitive(true)
        .build()
        .expect("IS_BASE64 regex")
});
pub static SIBLING_CONTENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"/\.( |$)/"#).expect("SIBLING_CONTENT regex"));
pub static BYLINE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"byline|author|dateline|writtenby|p-author"#)
        .case_insensitive(true)
        .build()
        .expect("BYLINE regex")
});
pub static NORMALIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\s{2,}"#).expect("NORMALIZE regex"));
pub static TOKENIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\W+"#).expect("TOKENIZE regex"));
pub static UNLIELY_CANDIDATES: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"-ad-|ai2html|banner|breadcrumbs|combx|community|cover-wrap|disqus|extra|footer|gdpr|header|legends|menu|related|remark|rss|shoutbox|sidebar|skyscraper|social|sponsor|supplemental|ad-break|agegate|pagination|pager|popup|yom-remote"#).case_insensitive(true).build().expect("UNLIELY_CANDIDATES regex")
});

pub static OKAY_MAYBE_ITS_A_CANDIDATE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"and|article|body|column|content|main|shadow|js-details-container|Box-header|user-repositories-list|repos-split-pane-content|read-only-cursor-text-area"#)
        .case_insensitive(true)
        .build()
        .expect("OKAY_MAYBE_ITS_A_CANDIDATE regex")
});
pub static HAS_CONTENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"/\S$/"#).expect("HAS_CONTENT regex"));
pub static HASH_URL: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^#.+"#).expect("HASH_URL regex"));

// js-details-container|Box-header|user-repositories-list|repos-split-pane-content|read-only-cursor-text-area
pub static POSITIVE: Lazy<Regex> =
    Lazy::new(|| {
        RegexBuilder::new(
        r#"article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story|headline|title|js-details-container|Box-header|user-repositories-list|repos-split-pane-content|read-only-cursor-text-area"#,
    ).case_insensitive(true).build()
    .expect("POSITIVE regex")
    });
pub static NEGATIVE: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|com-|contact|foot|footer|footnote|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|tool|widget"#).case_insensitive(true).build().expect("NEGATIVE regex")
});
pub static SHARE_ELEMENTS: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"(\b|_)(share|sharedaddy)(\b|_)"#)
        .case_insensitive(true)
        .build()
        .expect("SHARE_ELEMENTS regex")
});
pub static SRC_SET_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(\S+)(\s+[\d.]+[xw])?(\s*(?:,|$))"#).expect("SRC_SET_URL regex"));
pub static TITLE_SEPARATOR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#" [-|—\\/>»] "#).expect("TITLE_SEPARATOR regex"));
pub static TITLE_CUT_END: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"(.*)[-|—\\/>»] .*"#)
        .case_insensitive(true)
        .build()
        .expect("TITLE_CUT_END regex")
});
pub static WORD_COUNT: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\s+"#).expect("WORD_COUNT regex"));
pub static TITLE_CUT_FRONT: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"[^-|\\/>»]*[-|\\/>»](.*)"#)
        .case_insensitive(true)
        .build()
        .expect("TITLE_CUT_FRONT regex")
});
pub static VIDEOS: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"(www\.)?((dailymotion|youtube|youtube-nocookie|player\.vimeo|v\.qq)\.com|(archive|upload\.wikimedia)\.org|player\.twitch\.tv)"#).case_insensitive(true).build().expect("VIDEOS regex")
});
pub static BASE64_DATA_URL: Lazy<Regex> = Lazy::new(|| {
    RegexBuilder::new(r#"^data:\s*([^\s;,]+)\s*;\s*base64\s*,"#)
        .case_insensitive(true)
        .build()
        .expect("BASE64_DATA_URL regex")
});
pub const SCORE_ATTR: &str = "content_score";
pub const DATA_TABLE_ATTR: &str = "is_data_table";
pub const MINIMUM_TOPCANDIDATES: usize = 3;
pub const UNLIKELY_ROLES: &[&str] = &[
    "menu",
    "menubar",
    "complementary",
    "navigation",
    "alert",
    "alertdialog",
    "dialog",
];

pub const DEFAULT_TAGS_TO_SCORE: &[&str] = &[
    "SECTION", "H1", "H2", "H3", "H4", "H5", "H6", "P", "TD", "PRE", "TITLE",
];
pub const PRESENTATIONAL_ATTRIBUTES: &[&str] = &[
    "align",
    "background",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "frame",
    "hspace",
    "rules",
    "style",
    "valign",
    "vspace",
];
pub static DEPRECATED_SIZE_ATTRIBUTE_ELEMS: Lazy<HashSet<&str>> =
    Lazy::new(|| HashSet::from(["TABLE", "TH", "TD", "HR", "PRE"]));
pub static DIV_TO_P_ELEMS: Lazy<HashSet<&str>> = Lazy::new(|| {
    HashSet::from([
        "BLOCKQUOTE",
        "DL",
        "DIV",
        "IMG",
        "OL",
        "P",
        "PRE",
        "TABLE",
        "UL",
    ])
});

pub static VALID_EMPTY_TAGS: Lazy<HashSet<&str>> = Lazy::new(|| {
    HashSet::from([
        "AREA", "BASE", "BR", "COL", "EMBED", "HR", "IMG", "LINK", "META", "SOURCE", "TRACK",
    ])
});

pub static ALTER_TO_DIV_EXCEPTIONS: Lazy<HashSet<&str>> =
    Lazy::new(|| HashSet::from(["DIV", "ARTICLE", "SECTION", "P"]));

pub static EMBED_TAG_NAMES: Lazy<HashSet<&str>> =
    Lazy::new(|| HashSet::from(["OBJECT", "EMBED", "IFRAME"]));

pub const PHRASING_ELEMS: &[&str] = &[
    // "CANVAS", "IFRAME", "SVG", "VIDEO",
    "ABBR", "AUDIO", "B", "BDO", "BR", "BUTTON", "CITE", "CODE", "DATA", "DATALIST", "DFN", "EM",
    "EMBED", "I", "IMG", "INPUT", "KBD", "LABEL", "MARK", "MATH", "METER", "NOSCRIPT", "OBJECT",
    "OUTPUT", "PROGRESS", "Q", "RUBY", "SAMP", "SCRIPT", "SELECT", "SMALL", "SPAN", "STRONG",
    "SUB", "SUP", "TEXTAREA", "TIME", "VAR", "WBR",
];
