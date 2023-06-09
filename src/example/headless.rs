use anyhow::{anyhow, Result};
use headless_chrome::{types::PrintToPdfOptions, Browser, LaunchOptions};
use html2text::from_read;
use pdfium_render::prelude::*;
use readah::readability::Readability;
use serde_json;
use std::time::Duration;
use tiktoken_rs::cl100k_base;
use url::Url;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = "https://github.com/topics/leaderboard-api";

    let options = LaunchOptions {
        headless: true,
        // window_size: Some((1440, 2880)),
        window_size: Some((820, 1180)),
        ..Default::default()
    };

    let browser = Browser::new(options)?;

    let tab = browser.new_tab()?;

    // tab.set_default_timeout(Duration::from_secs(3));
    tab.navigate_to(url)?;
    tab.wait_until_navigated();

    // if let Ok(html) = tab.get_content() {
    //     // println!("{:?}", html.clone());
    //     let parsed_url = Url::parse(url)?;
    //     let scheme = parsed_url.scheme();
    //     let host = parsed_url.host_str().unwrap_or("");
    //     let base_url = Url::parse(&format!("{}://{}", scheme, host)).unwrap();
    //     match Readability::extract(&html, Some(base_url)).await {
    //         Ok(stripped_html) => {
    //             let output = from_read(stripped_html.to_string().as_bytes(), 80);
    //             // let output = stripped_html.to_string();
    //             println!("{:?}", output);
    //         }
    //         Err(_err) => {}
    //     }
    // }

    // let bpe = cl100k_base().unwrap();

    let pdf_options: Option<PrintToPdfOptions> = Some(PrintToPdfOptions {
        landscape: Some(false),
        display_header_footer: Some(false),
        print_background: Some(false),
        scale: Some(0.5),
        paper_width: Some(11.0),
        paper_height: Some(17.0),
        margin_top: Some(0.1),
        margin_bottom: Some(0.1),
        margin_left: Some(0.1),
        margin_right: Some(0.1),
        page_ranges: Some("1-2".to_string()),
        ignore_invalid_page_ranges: Some(true),
        prefer_css_page_size: Some(false),
        transfer_mode: None,
        ..Default::default()
    });

    let pdf_data = tab.print_to_pdf(pdf_options)?;

    let pdf_as_vec = pdf_data.to_vec();

    let text = Pdfium::new(
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
            "/Users/jaykchen/Downloads/pdfium-mac-arm64/lib/libpdfium.dylib",
        ))
        .or_else(|_| Pdfium::bind_to_system_library())?,
    )
    .load_pdf_from_byte_vec(pdf_as_vec, Some(""))?
    .pages()
    .iter()
    .map(|page| page.text().unwrap().all())
    .collect::<Vec<String>>()
    .join(" ");

    println!("{:?}", text);
    Ok(())
}
