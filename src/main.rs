use std::{fs::File, io::BufReader, str::FromStr, time::Duration};

use actix_web::middleware::{errhandlers::ErrorHandlers, Logger};
use actix_web::{web, App, HttpServer};
use rustls::{internal::pemfile, NoClientAuth, ServerConfig};
use sqlx::mysql::MySqlPoolOptions;

mod macros;
mod models;
mod web_handlers;

#[rustfmt::skip]
#[actix_web::main]
async fn main() -> std::io::Result<()> {
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
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("/etc/storagereloaded/config").required(false)).unwrap();
    settings.merge(config::File::with_name("config").required(false)).unwrap();
    settings.merge(config::Environment::with_prefix("APP")).unwrap();

    // Get port and host from config, or use the default port and host: 0.0.0.0:8081
    let host: String = settings.get_str("host").unwrap_or_else(|_| String::from("0.0.0.0"));
    let port: i64 = settings.get_int("port").unwrap_or(8081);
    let port: u16 = if port > (std::u16::MAX as i64) {
        eprintln!("Error: Port number can't be over 65535!");
        std::process::exit(1);
    } else {
        port as u16
    };

    // SSL config
    let use_ssl = settings.get_bool("ssl").unwrap_or(false);
    let cert_file = settings.get_str("cert_file").unwrap_or_else(|_| String::from("cert.pem"));
    let key_file = settings.get_str("key_file").unwrap_or_else(|_| String::from("key.pem"));

    // Static serving config
    let static_serving: bool = settings.get_bool("static_serving").unwrap_or(true);
    let static_dir: String = settings.get_str("static_dir").unwrap_or_else(|_| String::from("./static"));
    let index_file: String = settings.get_str("index_file").unwrap_or_else(|_| String::from("index.html"));

    // Database config
    let db_type = DbType::from_str(settings.get_str("db_type").expect("DB type is not specified!").as_str()).unwrap();
    let db_host = settings.get_str("db_host").expect("DB host is not specified!");
    let db_port = settings.get_int("db_port").unwrap_or(3306);
    let db_port: u16 = if db_port > (std::u16::MAX as i64) {
        eprintln!("Error: DB port number can't be over 65535!");
        std::process::exit(1);
    } else {
        db_port as u16
    };
    let db_user = settings.get_str("db_user").expect("DB user is not specified!");
    let db_password = settings.get_str("db_password").expect("DB password is not specified!");
    let db_database = settings.get_str("db_database").expect("DB database is not specified!");
    let db_url = format!(
        "{db_type}://{db_user}:{db_password}@{db_host}:{db_port}/{db_database}",
        db_type = db_type.to_string(),
        db_host = db_host,
        db_port = db_port,
        db_user = db_user,
        db_password = db_password,
        db_database = db_database
    );

    // Establish MySQL server connection (Pool with 4 connections, Timeout after 5 seconds)
    let pool = match MySqlPoolOptions::new().max_connections(4).connect_timeout(Duration::from_secs(5)).connect(&db_url).await {
        Ok(pool) => pool,

        // if that fails, print an error message and exit the programm
        Err(error) => {
            eprintln!("Error: {}", match error {
                sqlx::Error::Tls(msg) if msg.to_string().eq("InvalidDNSNameError") => "Insecure SQL server connection! Domain and specified host dosn't match!".to_string(),
                sqlx::Error::Tls(msg) => format!("TLS Error! {}", msg.to_string()),
                _ => error.to_string(),
            });
            std::process::exit(1); // Exit with error code 1
        },
    };

    // Setup server
    println!("Starting server on http://{host}:{port}", host = host, port = port);
    let mut server = HttpServer::new(move || {
        // Create a simple logger that writes all incomming requests to the console
        let logger = Logger::default();

        // Cross-Origin Requests
        let cors = actix_cors::Cors::default().allow_any_header().allow_any_origin().allow_any_method().max_age(3600);

        // Create a new App that handles all client requests
        let app = App::new()
            .wrap(logger)
            .wrap(cors)

            // If an internal error occures, 
            .wrap(ErrorHandlers::new().handler(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, web_handlers::sanitize_internal_error))

            // Provide a clone of the db pool
            // to enable services to access the database
            .data(pool.clone())

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
        // If the user dosn't need static serving, this step will be skipped
        if static_serving {
            app.service(actix_files::Files::new("/", &static_dir)
                .prefer_utf8(true)
                .index_file(index_file.as_str())
            )
        } else {
            app
        }
    });

    // Setup SSL
    server = if use_ssl {
        let mut config = ServerConfig::new(NoClientAuth::new());
        let cert_buf = &mut BufReader::new(File::open(cert_file).expect("Cannot read cert file!"));
        let key_buf = &mut BufReader::new(File::open(key_file).expect("Cannot read key file!"));

        let cert_chain = pemfile::certs(cert_buf).expect("Cannot parse cert file content!");
        let mut keys = pemfile::pkcs8_private_keys(key_buf).expect("Cannot parse key file content!");
        config.set_single_cert(cert_chain, keys.remove(0)).expect("Invalid key!");

        server.bind_rustls((host, port), config)?
    } else {
        server.bind((host, port))?
    };

    server.run().await
}

enum DbType {
    MySQL,
}

impl FromStr for DbType {
    type Err = config::ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "mysql" | "mariadb" => DbType::MySQL,
            _ => return Err(config::ConfigError::Message("Unsupported database type!".to_string())),
        })
    }
}

impl ToString for DbType {
    fn to_string(&self) -> String {
        match self {
            DbType::MySQL => "mysql",
        }
        .to_string()
    }
}
