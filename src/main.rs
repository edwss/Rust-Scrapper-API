use bollard::container::{Config, RemoveContainerOptions};
use bollard::service::{ContainerCreateResponse, CreateImageInfo, HostConfig, Mount};
use bollard::Docker;
use tokio::signal;
use warp::{http, Filter};

use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use futures_util::{stream::StreamExt, TryStreamExt};
use std::time::{Instant, Duration};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

const IMAGE: &str = "ultrafunk/undetected-chromedriver:latest";

pub struct Request {
    response: String,
    timeout: Instant
}

#[tokio::main]
async fn main() {
    let request_cache = Arc::new(Mutex::new(HashMap::<String, Request>::new()));
    let running_container = vizer_inicialization().await;

    let container_id = running_container.clone();
    let cache = request_cache.clone();
    let vizer_home = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("home"))
            .and(warp::path::end())
            .and_then(move || home(container_id.clone(), cache.clone())),
    );

    let container_id = running_container.clone();
    let cache = request_cache.clone();
    let vizer_search = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("search"))
            .and(
                warp::path::param()
                    .and_then(move |title: String| search(container_id.clone(), title.clone(), cache.clone())),
            )
            .and(warp::path::end()),
    );

    let container_id = running_container.clone();
    let cache = request_cache.clone();
    let vizer_parse = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("parse"))
            .and(warp::path("serie"))
            .and(warp::path("online"))
            .and(
                warp::path::param()
                    .and_then(move |title: String| parse(container_id.clone(), "serie".to_string(), title.clone(), cache.clone())),
            )
            .and(warp::path::end()),
    );

    let container_id = running_container.clone();
    let vizer_episodes = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("serie"))
            .and(warp::path("episodes"))
            .and(
                warp::path::param()
                    .and_then(move |title: String| episodes(container_id.clone(), title.clone())),
            )
            .and(warp::path::end()),
    );

    let container_id = running_container.clone();
    let vizer_episode_streaming = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("serie"))
            .and(warp::path("stream"))
            .and(
                warp::path::param()
                    .and_then(move |episode_id: String| streaming(container_id.clone(), episode_id.clone())),
            )
            .and(warp::path::end()),
    );

    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    let routes = vizer_home.or(vizer_search).or(vizer_parse).or(vizer_episodes).or(vizer_episode_streaming).or(hello);

    let serve_handler = warp::serve(routes);
    tokio::spawn(serve_handler.run(([0, 0, 0, 0], 3030)));

    match signal::ctrl_c().await {
        Ok(()) => {
            let container_id = running_container.clone();
            remove_container(container_id.to_string()).await;
            println!("Done")
        }
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    }
}

async fn vizer_inicialization() -> String {
    let _ = create_image().await.unwrap();
    let response = create_container().await.unwrap();

    let container_id = response.id.clone();

    let docker = Docker::connect_with_socket_defaults().unwrap();
    let _ = docker
        .start_container::<String>(&response.id, None)
        .await
        .unwrap();

    container_id
}

fn check_cache(request_cache: Arc<Mutex<HashMap<String, Request>>>, key: String) -> String{
    let cache = request_cache.lock().unwrap();
    if cache.contains_key(&key) {
        let value = cache.get(&key).unwrap();
        value.response.clone()
    } else {
        "".to_string()
    }
}

fn insert_cache(request_cache: Arc<Mutex<HashMap<String, Request>>>, key: String, value: Request) {
    let mut cache = request_cache.lock().unwrap();
    cache.insert(key, value);
}

async fn home(id: String, request_cache: Arc<Mutex<HashMap<String, Request>>>) -> Result<impl warp::Reply, warp::Rejection> {
    let response = check_cache(request_cache.clone(), "home".to_string());
    if !response.is_empty() {
        println!("Home : Cache hit");
        Ok(
            warp::reply::with_status(response, http::StatusCode::OK)
        )
    } else {
        println!("Home : Cache not hit");
        let docker = Docker::connect_with_socket_defaults().unwrap();
        let command = "/data/main_page.py";
        let response = exec(docker, &id, command, "").await.replace("'", "\"").to_string();
        insert_cache(request_cache.clone(), "home".to_string(), Request { response: response.clone(), timeout: Instant::now() });
        Ok(
            warp::reply::with_status(response, http::StatusCode::OK)
        )
    }
}

async fn search(id: String, title: String, request_cache: Arc<Mutex<HashMap<String, Request>>>) -> Result<impl warp::Reply, warp::Rejection> {
    let response = check_cache(request_cache.clone(), format!("search:{}", title));
    if !response.is_empty() {
        println!("Search : {} : Cache hit", title);
        Ok(
            warp::reply::with_status(response, http::StatusCode::OK)
        )
    } else {
        println!("Search : {} : Cache not hit", title);
        let docker = Docker::connect_with_socket_defaults().unwrap();
        let command = "/data/search.py";
        let response = exec(docker, &id, command, &title).await.replace("'", "\"").to_string();
        insert_cache(request_cache.clone(), format!("search:{}", title), Request { response: response.clone(), timeout: Instant::now() });
        Ok(warp::reply::with_status(
            response,
            http::StatusCode::OK,
        ))
    }
}

async fn parse(id: String, item_type: String, title: String, request_cache: Arc<Mutex<HashMap<String, Request>>>) -> Result<impl warp::Reply, warp::Rejection> {
    let response = check_cache(request_cache.clone(), format!("{}:{}", item_type, title));
    if !response.is_empty() {
        println!("Parse : {} : Cache hit", format!("{}:{}", item_type, title));
        Ok(
            warp::reply::with_status(response, http::StatusCode::OK)
        )
    } else {
        println!("Parse : {} : Cache not hit", format!("{}:{}", item_type, title));
        let docker = Docker::connect_with_socket_defaults().unwrap();
        let command = "/data/parse.py";
        let args = format!("{}/{}/{}", item_type, "online", title);
        let response = exec(docker, &id, command, &args).await.replace("'", "\"");
        insert_cache(request_cache.clone(), format!("{}:{}", item_type, title), Request { response: response.clone(), timeout: Instant::now() });
        Ok(warp::reply::with_status(
            response,
            http::StatusCode::OK,
        ))
    }
}

async fn episodes(id: String, season_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let docker = Docker::connect_with_socket_defaults().unwrap();
    let command = "/data/get_episodes.py";
    let response = exec(docker, &id, command, &season_id).await;
    Ok(warp::reply::with_status(
        response.replace("'", "\""),
        http::StatusCode::OK,
    ))
}

async fn streaming(id: String, episode_id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let docker = Docker::connect_with_socket_defaults().unwrap();
    let command = "/data/stream.py";
    let response = exec(docker, &id, command, &episode_id).await;
    Ok(warp::reply::with_status(
        response.replace("'", "\""),
        http::StatusCode::OK,
    ))
}

async fn create_image() -> Result<Vec<CreateImageInfo>, bollard::errors::Error> {
    let docker = Docker::connect_with_socket_defaults().unwrap();
    match docker
        .create_image(
            Some(CreateImageOptions {
                from_image: IMAGE,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await
    {
        Ok(response) => return Ok(response),
        Err(err) => return Err(err),
    }
}

async fn create_container() -> Result<ContainerCreateResponse, bollard::errors::Error> {
    let c_1gb = 1073741824; //1 GB in bytes
    let docker = Docker::connect_with_socket_defaults().unwrap();

    let mut mounts = Vec::<Mount>::new();
    mounts.push(Mount {
        target: Some("/data".to_string()),
        source: Some("scrapper-rust".to_string()),
        typ: Some(bollard::service::MountTypeEnum::VOLUME),
        read_only: Some(false),
        ..Default::default()
    });

    let container_config = Config {
        image: Some(IMAGE),
        tty: Some(true),
        entrypoint: Some(vec!["./entrypoint.sh", "bash"]),
        host_config: Some(HostConfig {
            mounts: Some(mounts),
            shm_size: Some(c_1gb),
            ..Default::default()
        }),
        ..Default::default()
    };

    match docker
        .create_container::<&str, &str>(None, container_config)
        .await
    {
        Ok(it) => return Ok(it),
        Err(err) => return Err(err),
    }
}

async fn remove_container(id: String) {
    let docker = Docker::connect_with_socket_defaults().unwrap();
    let _ = docker
        .remove_container(
            &id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

async fn exec(docker: Docker, id: &str, command: &str, arg: &str) -> String {
    let exec = docker
        .create_exec(
            &id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["python", command, arg]),
                ..Default::default()
            },
        )
        .await;

    let mut response: String = "".to_string();
    if let StartExecResults::Attached { mut output, .. } =
        docker.start_exec(&exec.unwrap().id, None).await.unwrap()
    {
        while let Some(Ok(msg)) = output.next().await {
            response = response + &msg.to_string();
        }
    } else {
        unreachable!();
    }

    response
}
