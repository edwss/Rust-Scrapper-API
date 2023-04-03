use bollard::container::{Config, RemoveContainerOptions};
use bollard::service::{ContainerCreateResponse, CreateImageInfo, HostConfig, Mount};
use bollard::Docker;
use futures::future;
use std::error::Error;
use tokio::signal;
use warp::{http, Filter};

use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use futures_util::{stream::StreamExt, TryStreamExt};

const IMAGE: &str = "ultrafunk/undetected-chromedriver:latest";

#[tokio::main]
async fn main() {
    let docker = Docker::connect_with_socket_defaults().unwrap();
    create_image(docker.clone()).await;
    let response = create_container(docker.clone()).await.unwrap();
    let _ = docker.start_container::<String>(&response.id, None).await;
    let container_id = &response.id.clone();

    let vizer_home = warp::get().and(
        warp::path("vizer")
            .and(warp::path("v1"))
            .and(warp::path("home"))
            .and(warp::path::end())
            .and_then(move || home_page(docker.clone(), response.id.clone())),
    );


    // // GET /hello/warp => 200 OK with body "Hello, warp!"
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    let routes = vizer_home.or(hello);

    let serve_handler = warp::serve(routes);
    tokio::spawn(serve_handler.run(([0, 0, 0, 0], 3030)));

    match signal::ctrl_c().await {
        Ok(()) => {
            remove_container(container_id.to_string()).await;
            println!("Done")
        }
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
}

async fn home_page(docker: Docker, id: String) -> Result<impl warp::Reply, warp::Rejection> {
    let response = exec(docker, &id).await;
    Ok(warp::reply::with_status(
        response.replace("'", "\""),
        http::StatusCode::OK,
    ))
}

async fn create_image(docker: Docker) {
    _ = docker
        .create_image(
            Some(CreateImageOptions {
                from_image: IMAGE,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await;
}

async fn create_container(
    docker: Docker,
) -> Result<ContainerCreateResponse, bollard::errors::Error> {
    let mut mounts = Vec::<Mount>::new();
    mounts.push(Mount {
        target: Some("/data".to_string()),
        source: Some("scrapper-rust".to_string()),
        typ: Some(bollard::service::MountTypeEnum::VOLUME),
        read_only: Some(false),
        ..Default::default()
    });
    let alpine_config = Config {
        image: Some(IMAGE),
        tty: Some(true),
        entrypoint: Some(vec!["./entrypoint.sh", "bash"]),
        host_config: Some(HostConfig {
            mounts: Some(mounts),
            shm_size: Some(1073741824),
            ..Default::default()
        }),
        ..Default::default()
    };

    match docker
        .create_container::<&str, &str>(None, alpine_config)
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

async fn exec(docker: Docker, id: &str) -> String {
    let exec = docker
        .create_exec(
            &id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["python", "/data/main_page.py"]),
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
