use failure::Error;
use futures::prelude::*;
use log::*;
use serde::Deserialize;
use std::time::{Duration, Instant};

// dslreport's test key
const API_KEY: &str = "12345678";

#[derive(Debug, Deserialize)]
pub struct DSLReportsResponse {
  pub locations: Vec<String>,
  pub plat: String,
  pub server_rids: Vec<String>,
  pub locations_short: Vec<String>,
  pub key: String,
  pub ports: Vec<String>,
  pub prefs: DSLReportsResponsePrefs,
  pub dnsdom: Option<String>,
  pub servers: Vec<String>,
  pub ipaddr: String,
}

#[derive(Debug, Deserialize)]
pub struct DSLReportsResponsePrefs {
  pub ipv6: u8,
  pub https: u8,
}

pub async fn get_server_config() -> Result<DSLReportsResponse, Error> {
  info!("get_server_config");

  Ok(
    reqwest::get(&format!(
      "https://api.dslreports.com/speedtest/1.0/?typ=p&plat=10&apikey={}",
      API_KEY
    ))
    .await?
    .json()
    .await?,
  )
}

#[tokio::test]
async fn test_get_server_config() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  let response = get_server_config().await.unwrap();
  info!("json: {:#?}", response);
}

pub async fn get_ping_from_server(server: String) -> Result<Duration, Error> {
  let start_time = Instant::now();
  // head method??
  reqwest::get(&format!("{}/front/0k", server)).await?;
  let end_time = Instant::now();

  Ok(end_time - start_time)
}

/// ping every server in `.servers` and sort by nanoseconds ping
pub async fn get_servers_sorted_by_ping() -> Result<Vec<(String, Duration)>, Error> {
  info!("get_servers_sorted_by_ping");

  let response = get_server_config().await?;

  let mut pings: Vec<_> = stream::iter(response.servers)
    .map(|server| async { (server.clone(), get_ping_from_server(server).await) })
    .buffer_unordered(4)
    .filter_map(|(server, result)| async { result.ok().map(|value| (server, value)) })
    .collect()
    .await;

  pings.sort_unstable_by(|(_, ping1), (_, ping2)| ping1.partial_cmp(&ping2).unwrap());

  Ok(pings)
}

#[tokio::test]
async fn test_get_servers_sorted_by_ping() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  let pings = get_servers_sorted_by_ping().await.unwrap();
  info!("sorted latency: {:#?}", pings);
}

/// returns a Future<Stream> where the stream will output (bytes, duration) every `min_update_interval`
pub async fn get_download_speed_stream(
  server: String,
  min_update_interval: Duration,
) -> Result<impl Stream<Item = Result<(usize, Duration), Error>>, Error> {
  info!("get_download_speed_stream {}", server);

  let stream = reqwest::get(&format!("{}/front/k", server))
    .await?
    .bytes_stream();

  let lock = std::sync::Arc::new(futures::lock::Mutex::new((0, Instant::now())));

  Ok(
    stream
      .map_err(|err| err.into())
      .try_filter_map(move |bytes| {
        let lock = lock.clone();

        async move {
          let mut guard = lock.lock().await;

          let (mut last_bytes, mut last_now) = *guard;

          let now = Instant::now();
          let duration = now - last_now;

          last_bytes += bytes.len();

          let value = if duration >= min_update_interval {
            let new_value = (last_bytes, duration);

            last_now = now;
            last_bytes = 0;

            Some(new_value)
          } else {
            None
          };

          *guard = (last_bytes, last_now);

          Ok(value)
        }
      })
      .boxed(),
  )
}

#[tokio::test]
async fn test_get_download_speed_stream() {
  env_logger::Builder::from_default_env()
    .filter(Some("dslreports"), log::LevelFilter::Info)
    .init();

  let servers = get_servers_sorted_by_ping().await.unwrap();
  let server = &servers[0].0;

  let mut stream = get_download_speed_stream(server.to_string(), Duration::from_secs(1))
    .await
    .unwrap();

  while let Some(result) = stream.next().await {
    let (bytes, duration) = result.unwrap();
    let rate = ((bytes as f64) / 1_000_000.0) / duration.as_secs_f64();

    info!("{} MB/s", rate);
  }
}
