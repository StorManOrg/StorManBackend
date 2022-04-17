use std::{fs::File, io::BufReader, time::Duration};

use actix_web::http::StatusCode;
use actix_web::middleware::{ErrorHandlers, Logger};
use actix_web::{web, App, HttpServer};
use rustls::ServerConfig;
use sqlx::mysql::MySqlPoolOptions;

mod macros;
mod models;
mod web_handlers;

#[rustfmt::skip]
async fn run() -> Result<(), String> {
    // Setup logger
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    if std::env::var("RUST_BACKTRACE").is_err() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    env_logger::init();

    // Load user preferences from config file and environment.
    // Environment variables override the config file!
    let settings = config::Config::builder()
        .add_source(config::File::with_name("/etc/storagereloaded/config").required(false))
        .add_source(config::File::with_name("config").required(false))
        .add_source(config::Environment::with_prefix("APP"))
        .build().map_err(|err| err.to_string())?;

    // Get port and host from config, or use the default port and host: 0.0.0.0:8081
    let host: String = settings.get_string("host").unwrap_or_else(|_| String::from("0.0.0.0"));
    let port: u16 = settings.get_int("port").unwrap_or(8081).try_into().map_err(|_| "Port number can't be over 65535!")?;

    // SSL config
    let use_ssl = settings.get_bool("ssl").unwrap_or(false);
    let cert_file = settings.get_string("cert_file").unwrap_or_else(|_| String::from("cert.pem"));
    let key_file = settings.get_string("key_file").unwrap_or_else(|_| String::from("key.pem"));

    // Static serving config
    let static_serving: bool = settings.get_bool("static_serving").unwrap_or(true);
    let static_dir: String = settings.get_string("static_dir").unwrap_or_else(|_| String::from("./static"));
    let index_file: String = settings.get_string("index_file").unwrap_or_else(|_| String::from("index.html"));

    // Workers
    let num_workers: usize = settings.get_int("workers").unwrap_or(2).try_into().map_err(|_| "Too many workers!")?;
    let num_connections: u32 = settings.get_int("pool_connections")
        .unwrap_or_else(|_| num_workers.try_into().unwrap_or(2))
        .try_into().map_err(|_| "Too many connections!")?;

    // Database config
    let db_type = settings.get_string("db_type").map_err(|_| "DB type is not specified!")?;
    if !db_type.eq_ignore_ascii_case("mysql") {
        return Err("Unsupported database type".to_string());
    }

    let db_host = settings.get_string("db_host").map_err(|_| "DB host is not specified!")?;
    let db_port: u16 = settings.get_int("db_port").unwrap_or(3306).try_into().map_err(|_| "DB port number can't be over 65535!")?;
    let db_user = settings.get_string("db_user").map_err(|_| "DB user is not specified!")?;
    let db_password = settings.get_string("db_password").map_err(|_| "DB password is not specified!")?;
    let db_database = settings.get_string("db_database").map_err(|_| "DB database is not specified!")?;
    let db_url = format!("{db_type}://{db_user}:{db_password}@{db_host}:{db_port}/{db_database}");

    // Establish MySQL server connection (Timeout after 15 seconds)
    let pool = MySqlPoolOptions::new()
        .max_connections(num_connections)
        .connect_timeout(Duration::from_secs(15))
        .connect(&db_url)
        .await
        .map_err(|err| match err {
            sqlx::Error::Tls(msg) if msg.to_string().eq("InvalidDNSNameError") => "Insecure SQL server connection! Domain and specified host don't match!".to_string(),
            sqlx::Error::Tls(msg) => format!("TLS Error! {}", msg),
            _ => err.to_string(),
        })?;

    // Setup server
    println!("Starting server on http://{host}:{port}", host = host, port = port);
    let mut server = HttpServer::new(move || {
        // Create a simple logger that writes all incoming requests to the console
        let logger = Logger::default();

        // Cross-Origin Requests
        let cors = actix_cors::Cors::default().allow_any_header().allow_any_origin().allow_any_method().max_age(3600);

        // Create a new App that handles all client requests
        let mut app = App::new()
            .wrap(logger)
            .wrap(cors)

            // If an internal error occurs, remove the sensetive content from the response
            .wrap(ErrorHandlers::new().handler(StatusCode::INTERNAL_SERVER_ERROR, web_handlers::sanitize_internal_error))

            // Provide a clone of the reference to the db pool
            // to enable services to access the database
            .app_data(actix_web::web::Data::new(pool.clone()))

            // If the user wants to serve static files (in addition to the api),
            // move the api to a sub layer: '/' => '/api'
            .service(web::scope(if static_serving { "/api" } else { "/" })
                //.guard(guard::Header("Content-Type", "application/json"))
                .default_service(web::route().to(web_handlers::not_implemented))
                .service(web_handlers::get_system_info)
                .service(web_handlers::teapod)
                .service(web::scope("/v1")
                    .default_service(web::route().to(web_handlers::not_implemented))

                    // Open access
                    .service(web_handlers::auth::get_post_auth)

                    // Restricted access
                    .service(web_handlers::auth::delete_auth)
                    .service(web_handlers::item::get_items)
                    .service(web_handlers::item::get_item)
                    .service(web_handlers::item::put_item)
                    .service(web_handlers::item::update_item)
                    .service(web_handlers::item::delete_item)
                    .service(web_handlers::tag::get_tags)
                    .service(web_handlers::tag::get_tag)
                    .service(web_handlers::tag::put_tag)
                    .service(web_handlers::tag::update_tag)
                    .service(web_handlers::tag::delete_tag)
                    .service(web_handlers::database::get_databases)
                    .service(web_handlers::database::get_database)
                    .service(web_handlers::database::put_database)
                    .service(web_handlers::database::update_database)
                    .service(web_handlers::database::delete_database)
                    .service(web_handlers::location::get_locations)
                    .service(web_handlers::location::get_location)
                    .service(web_handlers::location::put_location)
                    .service(web_handlers::location::update_location)
                    .service(web_handlers::location::delete_location)
                )
            );

        // After registering the api services, register the static file service.
        // If the user doesn't need static serving, this step will be skipped
        if static_serving {
            app = app.service(actix_files::Files::new("/", &static_dir)
                .prefer_utf8(true)
                .index_file(index_file.as_str())
            )
        };

        app
    });

    // Setup SSL
    server = if use_ssl {
        let cert_buf = &mut BufReader::new(File::open(cert_file).map_err(|_| "Cannot read cert file!")?);
        let key_buf = &mut BufReader::new(File::open(key_file).map_err(|_| "Cannot read key file!")?);

        let cert_chain = rustls_pemfile::certs(cert_buf)
            .map_err(|_| "Cannot parse cert file content!")?
            .into_iter().map(rustls::Certificate).collect();
        let mut keys: Vec<rustls::PrivateKey> = rustls_pemfile::pkcs8_private_keys(key_buf)
            .map_err(|_| "Cannot parse key file content!")?
            .into_iter().map(rustls::PrivateKey).collect();

        // Exit if no keys could be parsed
        if keys.is_empty() {
            return Err("Could not locate PKCS 8 private keys".to_string());
        }

        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, keys.remove(0))
            .map_err(|err| err.to_string())?;

        server.bind_rustls((host, port), config).map_err(|err| err.to_string())?
    } else {
        server.bind((host, port)).map_err(|err| err.to_string())?
    };

    server.workers(num_workers).run().await.map_err(|err| err.to_string())
}

#[actix_web::main]
async fn main() {
    let result = run().await;

    std::process::exit(match result {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("[Error] {}", error);
            1
        }
    });
}
