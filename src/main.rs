use warp::{http, Filter};

#[tokio::main]
async fn main() {
    let vizer_home = warp::get()
        .and(warp::path("vizer")
        .and(warp::path("v1"))
        .and(warp::path("home"))
        .and(warp::path::end())
        .and_then(home_page));

    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    let routes = vizer_home.or(hello);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn home_page() -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::with_status("OK", http::StatusCode::OK))
}
