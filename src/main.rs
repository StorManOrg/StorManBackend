use actix_files::Files;
use actix_web::{get, web, App, HttpRequest, HttpServer, Result};
use std::collections::HashMap;

mod models;
use models::Item;

mod web_handler;

#[get("/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = req
        .match_info()
        .query("item_id")
        .parse()
        .expect("Not a number");

    Ok(web::Json(Item {
        id: Some(item_id),
        name: String::from("Test Item 3"),
        description: String::from("Sample Description"),
        image: String::from("fejfeifji"),
        location: 5,
        tags: vec![6, 3],
        amount: 29,
        properties_internal: vec![],
        properties_custom: vec![],
        attachments: HashMap::new(),
        last_edited: 637463746,
        created: 989343,
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load user preferences from config file and environment
    let mut settings = config::Config::default();
    settings
        .merge(config::File::with_name("config").required(false))
        .unwrap();
    settings
        .merge(config::Environment::with_prefix("APP"))
        .unwrap();

    // Get port from config, or use the default: 8081
    let port: i64 = settings.get_int("port").unwrap_or(8081);
    let port: u16 = if port > (std::u16::MAX as i64) {
        panic!("Port number dosn't fit into an u16!");
    } else {
        port as u16
    };

    println!("Starting server on http://127.0.0.1:{}", port);
    HttpServer::new(move || {
        App::new()
            .service(
                web::scope("/api")
                    .service(web_handler::get_system_info)
                    .service(web::scope("v1").service(get_item)),
            )
            .service(
                Files::new("/", "./static")
                    .prefer_utf8(true)
                    .index_file("index.html"),
            )
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}
