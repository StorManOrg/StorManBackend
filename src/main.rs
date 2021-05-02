use actix_files::Files;
use actix_web::{get, App, HttpRequest, HttpServer, Result};

#[get("/api/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<String> {
    let item_id: u32 = req
        .match_info()
        .query("item_id")
        .parse()
        .expect("Not a number");

    Ok(format!("You requested item id {}!", item_id))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .service(get_item)
            .service(Files::new("/", "./static"))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
