mod readability;
use read::readability::Readability;
use html2text::from_read;
use std::path::Path;
use std::{path::PathBuf, process::exit};

use anyhow::Result;
// use article_scraper::{
//     ArticleScraper, FtrConfigEntry, FullTextParser,
//     Readability::{self},
// };

use http_req::request;
use reqwest::header::HeaderMap;
use reqwest::Client;
use std::{fs, println};
use tokio::sync::mpsc::{self, Sender};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    let url = "https://hackaday.com/2023/06/01/farewell-american-computer-magazines/";

    let parsed_url = Url::parse(url)?;
    let scheme = parsed_url.scheme();
    let host = parsed_url.host_str().unwrap_or("");
    let base_url = Url::parse(&format!("{}://{}", scheme, host)).unwrap();

    let mut writer = Vec::new(); //container for body of a response
    let res = request::get(url, &mut writer).unwrap();
    match Readability::extract(&String::from_utf8(writer).unwrap(), Some(base_url)).await {
        Ok(res) => {
            let output = from_read(res.to_string().as_bytes(), 80);

            println!("{:}", output);
        }
        Err(_err) => {}
    }

    Ok(())
}
