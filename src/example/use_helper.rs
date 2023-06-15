use readah::readability::helper::*;
use std::env;
#[tokio::main]
async fn main() {
    // let url = "https://example.com/";
    let args: Vec<String> = env::args().collect();
    let url = args.into_iter().nth(1).unwrap();

    if let Ok(text) = text_to_use(&url).await {
        println!("{:?}", text);
    }
}
