use anyhow::Result;
use headless_chrome::{types::PrintToPdfOptions, Browser, LaunchOptions};
use pdfium_render::prelude::*;

// use headless Chrome to fetch and render webpages, most of which spread with js all over
// capture webpage in pdf format in memory, which is a trick to preserve
// the spatial context of the texts on the page and make the extraction of
// human viewable text easier
// this code aims to get all human viewable texts on a webpage, instead of one central article
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = "https://github.com/topics/leaderboard-api";
    // set the headless Chrome to open a webpage in portrait mode of certain width and height
    // here in an iPad resolution, is a way to pursuade webserver to send less non-essential
    // data, and make the virtual browser to show the central content, for websites
    // with responsive design, with less clutter
    let options = LaunchOptions {
        headless: true,
        window_size: Some((820, 1180)),
        ..Default::default()
    };

    let browser = Browser::new(options)?;

    let tab = browser.new_tab()?;

    // tab.set_default_timeout(Duration::from_secs(3));
    tab.navigate_to(url)?;
    tab.wait_until_navigated()?;

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
    //code below uses dynamically linked libpdfium.dylib on a M1 Mac
    //it takes some efforts to bind libpdfium on different platforms
    //please visit https://github.com/ajrcarey/pdfium-render/tree/master
    //for more details
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
