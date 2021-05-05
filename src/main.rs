use actix_files::Files;
use actix_web::{get, web, App, HttpRequest, HttpServer, Result};
use std::collections::HashMap;

mod models;

#[get("/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<web::Json<models::Item>> {
    let item_id: u64 = req
        .match_info()
        .query("item_id")
        .parse()
        .expect("Not a number");

    Ok(web::Json(models::Item {
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
    HttpServer::new(move || {
        App::new()
            .service(web::scope("/api").service(get_item))
            .service(Files::new("/", "./static"))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
