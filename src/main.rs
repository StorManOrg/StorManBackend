use actix_files::Files;
use actix_web::middleware::{self, Logger};
use actix_web::{guard, web, App, HttpServer};

mod macros;
mod models;
mod web_handler;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "info");
    std::env::set_var("RUST_BACKTRACE", "1");
    env_logger::init();

    // Load user preferences from config file and environment.
    // Environment variables override the config file!
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("config").required(false)).unwrap();
    settings.merge(config::Environment::with_prefix("APP")).unwrap();

    // Get port and host from config, or use the default port and host: 0.0.0.0:8081
    let host: String = settings.get_str("host").unwrap_or_else(|_| String::from("0.0.0.0"));
    let port: i64 = settings.get_int("port").unwrap_or(8081);
    let port: u16 = if port > (std::u16::MAX as i64) {
        panic!("Port number dosn't fit into an u16!");
    } else {
        port as u16
    };

    let static_serving: bool = settings.get_bool("static_serving").unwrap_or(true);
    let index_file: String = settings.get_str("index_file").unwrap_or_else(|_| String::from("index.html"));

    println!("Starting server on http://{host}:{port}", host = host, port = port);
    HttpServer::new(move || {
        // Create a simple logger that writes all incomming requests to the console
        let logger = Logger::default();

        // Create a new App that handles all client requests
        let app = App::new()
            .wrap(logger)
            .wrap(middleware::DefaultHeaders::new().header("Access-Control-Allow-Origin", "*"))
            .wrap(middleware::DefaultHeaders::new().header("Access-Control-Allow-Headers", "*"))
            // If the user wants to serve static files (in addition to the api),
            // move the api to a sub layer: '/' => '/api'
            .service(web::scope(if static_serving { "/api" } else { "/" })
                //.guard(guard::Header("Content-Type", "application/json"))
                .service(web_handler::get_system_info)
                .service(web::scope("/v1")
                    .service(web_handler::get_auth)
                    .service(web::scope("/")
                        // make sure that only authorized users can access the following services
                        .guard(guard::fn_guard(|req| req.headers().contains_key("X-StoRe-Session")))
                        .guard(guard::fn_guard(web_handler::auth_guard))
                        .service(web_handler::get_items)
                        .service(web_handler::get_items)
                        .service(web_handler::get_item)
                        .service(web_handler::create_item)
                        .service(web_handler::delete_item)
                        .service(web_handler::get_tags)
                        .service(web_handler::create_tag)
                        .service(web_handler::delete_tag)
                        .service(web_handler::get_tag)
                    )
                )
        );

        // After registering the api services, register the static file service.
        // If the user dosn't need static serving, this step will be skipped
        if static_serving {
            app.service(Files::new("/", "./static")
                .prefer_utf8(true)
                .index_file(index_file.as_str())
            )
        } else {
            app
        }
    })
    .bind((host, port))?
    .run()
    .await
}
