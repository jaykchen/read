use html2text::from_read;
use readah::readability::Readability;
use anyhow::Result;
use http_req::request;
use url::Url;


#[tokio::main]
async fn main() -> Result<()> {
    let url = "https://2023.fossy.us/";

    let parsed_url = Url::parse(url)?;
    let scheme = parsed_url.scheme();
    let host = parsed_url.host_str().unwrap_or("");
    let base_url = Url::parse(&format!("{}://{}", scheme, host)).unwrap();

    let mut writer = Vec::new(); //container for body of a response
    let res = request::get(url, &mut writer).unwrap();
    match Readability::extract(&String::from_utf8(writer).unwrap(), Some(base_url)).await {
        Ok(res) => {
            let output = from_read(res.to_string().as_bytes(), 80);

            let head = output.lines().take(100).collect::<Vec<&str>>().join("");
            println!("{:}", head);
        }
        Err(_err) => {}
    }

    Ok(())
}
