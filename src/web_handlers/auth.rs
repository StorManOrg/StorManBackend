use std::{collections::HashMap, pin::Pin};

use actix_web::{error, web, FromRequest, HttpRequest, HttpResponse};
use futures::future;
use sqlx::{MySqlPool, Row};

use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::collection;
use crate::models::{AuthedUser, UserCredentials};

#[rustfmt::skip]
#[actix_web::route("/auth", method="GET", method="POST")]
async fn get_post_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> actix_web::Result<HttpResponse> {
    println!("{:?}", req);
    // Query for the user_id with the credentials from the request
    let query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT id FROM users WHERE username = ? AND password = ?")
        .bind(&req.username)
        .bind(&req.password)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the user was found and extract the user id,
    // if there was no row found, return an forbidden error (code 403).
    let user_id: u64 = match query {
        Ok(row) => row.get(0),
        Err(error) => return Err(match error {
            sqlx::Error::RowNotFound => error::ErrorForbidden("invalid username or password!"),
            _ => error::ErrorInternalServerError(error),
        }),
    };

    // Generate a unique session_id and save it in the database.
    // We need a infinite loop here because we want to make sure,
    // that we don't get a duplicate.
    let session_id: String = loop {
        // Generate 8 random alphanumeric (a-z,A-Z,0-9) characters.
        let session_id: String = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();

        // Try to insert that into the sessions sql table...
        let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO sessions VALUES (?, ?, CURRENT_TIMESTAMP(), CURRENT_TIMESTAMP())")
            .bind(&session_id)
            .bind(&user_id)
            .execute(pool.as_ref())
            .await;

        // ... and check if it succeeded.
        match query {
            Ok(_) => break session_id,

            // If not, try it again (but only if the error occurred because of a duplicate).
            Err(error) => {
                return Err(match error {
                    sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => continue,
                    _ => error::ErrorInternalServerError(error),
                });
            }
        }
    };

    let map: HashMap<String, String> = collection! {
        "session_id".to_string() => session_id
    };
    Ok(HttpResponse::Created().json(map))
}

#[actix_web::delete("/auth")]
async fn delete_auth(pool: web::Data<MySqlPool>, session: AuthedUser) -> actix_web::Result<HttpResponse> {
    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM sessions WHERE session_id = ?")
        .bind(&session.session_id)
        .execute(pool.as_ref())
        .await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the session didn't even exist!
    // Technically this can't happen, because we made sure
    // the user's session is valid before we even entered
    // this function. (See #AuthedUser for more)
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorForbidden("invalid session id!"));
    }

    Ok(HttpResponse::Ok().finish())
}

impl FromRequest for AuthedUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn futures::Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        // We need to clone the pool here because the sql operation later on
        // are async and the compiler can't guarantee us that lifetime of the reference.
        // This only clones the pointer to the pool and NOT the pool itself.
        let pool = req.app_data::<web::Data<MySqlPool>>().unwrap().clone();
        let session_id = match req.headers().get("X-StoRe-Session") {
            Some(header) => match header.to_str() {
                Ok(session_id) => session_id.to_string(),
                Err(_) => return Box::pin(future::err(error::ErrorBadRequest("invalid characters in session id!"))),
            },
            None => return Box::pin(future::err(error::ErrorBadRequest("session id is missing!"))),
        };

        // We need a pinned boxes here because
        // we're using different return types.
        Box::pin(async move {
            let query: Result<AuthedUser, sqlx::Error> = sqlx::query_as::<_, AuthedUser>("SELECT session_id FROM sessions WHERE session_id = ?")
                .bind(&session_id)
                .fetch_one(pool.as_ref())
                .await;

            match query {
                Ok(auth) => Ok(auth),
                Err(error) => Err(match error {
                    sqlx::Error::RowNotFound => error::ErrorForbidden("invalid session id!"),
                    _ => error::ErrorInternalServerError(error),
                }),
            }
        })
    }
}
