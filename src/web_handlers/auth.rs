use std::{collections::HashMap, pin::Pin};

use actix_web::{error, web, FromRequest, HttpRequest, HttpResponse};
use sqlx::{MySqlPool, Row};

use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::collection;
use crate::models::{AuthedUser, UserCredentials};

#[actix_web::route("/auth", method = "GET", method = "POST")]
async fn get_post_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> actix_web::Result<HttpResponse> {
    let mut connection = pool.acquire().await.map_err(error::ErrorInternalServerError)?;

    // Query for the user_id with the credentials from the request
    let query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT id FROM users WHERE username = ? AND password = ?")
        .bind(&req.username)
        .bind(&req.password)
        .fetch_one(&mut connection)
        .await;

    // Check if the user was found and extract the user id,
    // if there was no row found, return an forbidden error (code 403).
    let user_id: u64 = query
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => error::ErrorForbidden("invalid username or password!"),
            _ => error::ErrorInternalServerError(err),
        })?
        .get(0);

    // Generate a unique session_id and save it in the database.
    // We need a infinite loop here because we want to make sure,
    // that we don't get a duplicate.
    let session_id: String = loop {
        // Generate 8 random alphanumeric (a-z,A-Z,0-9) characters.
        let session_id: String = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();

        // Try to insert that into the sessions sql table...
        let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO sessions VALUES (?, ?, CURRENT_TIMESTAMP(), CURRENT_TIMESTAMP())")
            .bind(&session_id)
            .bind(&user_id)
            .execute(&mut connection)
            .await;

        // If the query failed, try it again (but only if the error occurred because of a duplicate).
        if let Err(error) = insertion_query {
            match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => continue,
                _ => return Err(error::ErrorInternalServerError(error)),
            };
        }

        break session_id;
    };

    let map: HashMap<&str, String> = collection! {
        "session_id" => session_id
    };
    Ok(HttpResponse::Created().json(map))
}

#[actix_web::delete("/auth")]
async fn delete_auth(pool: web::Data<MySqlPool>, session: AuthedUser) -> actix_web::Result<HttpResponse> {
    let mut connection = pool.acquire().await.map_err(error::ErrorInternalServerError)?;

    let query: sqlx::mysql::MySqlQueryResult = sqlx::query("DELETE FROM sessions WHERE session_id = ?")
        .bind(&session.session_id)
        .execute(&mut connection)
        .await
        .map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the session didn't even exist!
    // Technically this can't happen, because we made sure
    // the user's session is valid before we even entered
    // this function. (See #AuthedUser for more)
    if query.rows_affected() == 0 {
        return Err(error::ErrorForbidden("invalid session id!"));
    }

    Ok(HttpResponse::Ok().finish())
}

impl FromRequest for AuthedUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn futures::Future<Output = Result<Self, Self::Error>>>>;

    // Because async trait functions are currently
    // not supported, we need to return a Future.
    // And since Futures cannot be moved in memory
    // we need to pin them.
    fn from_request(req_ref: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        // We need to clone the request here because the sql operation in this function
        // is async and the compiler can't guarantee us lifetime that lasts long enough.
        // This only clones the pointer to the request and NOT the request itself.
        let req = req_ref.clone();

        Box::pin(async move {
            let session_id = req
                .headers()
                .get("X-StoRe-Session")
                .ok_or_else(|| error::ErrorBadRequest("session id is missing!"))?
                .to_str()
                .map_err(|_| error::ErrorBadRequest("invalid characters in session id!"))?;

            let pool = req
                .app_data::<web::Data<MySqlPool>>()
                .ok_or_else(|| error::ErrorInternalServerError("could not clone sqlx pool"))?;

            let mut connection = pool.acquire().await.map_err(error::ErrorInternalServerError)?;

            let query: Result<AuthedUser, sqlx::Error> = sqlx::query_as::<_, AuthedUser>("SELECT session_id, user_id FROM sessions WHERE session_id = ?")
                .bind(&session_id)
                .fetch_one(&mut connection)
                .await;

            query.map_err(|err| match err {
                sqlx::Error::RowNotFound => error::ErrorForbidden("invalid session id!"),
                _ => error::ErrorInternalServerError(err),
            })
        })
    }
}
