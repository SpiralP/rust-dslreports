use failure::Error;
use futures::stream;
use hyper::{
  self,
  client::HttpConnector,
  rt::{lazy, Future, Stream},
  Client,
};
use hyper_tls::HttpsConnector;
use log::*;
use serde::Deserialize;
use serde_json;
use time::precise_time_ns;

const API_KEY: &str = "12345678"; // test key

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

fn new_client() -> impl Future<Item = Client<HttpsConnector<HttpConnector>>, Error = Error> {
  lazy(|| {
    let https = HttpsConnector::new(4)?;
    let client = Client::builder().build::<_, hyper::Body>(https);
    Ok(client)
  })
}

pub fn get_server_config() -> impl Future<Item = DSLReportsResponse, Error = Error> {
  info!("get_server_config");

  new_client().and_then(|client| {
    client
      .get(
        format!(
          "https://api.dslreports.com/speedtest/1.0/?typ=p&plat=10&apikey={}",
          API_KEY
        )
        .parse()
        .unwrap(),
      )
      .and_then(|response| response.into_body().concat2())
      .from_err()
      .and_then(|body| {
        let json: DSLReportsResponse = serde_json::from_slice(&body)?;

        Ok(json)
      })
  })
}

#[test]
fn test_get_server_config() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  hyper::rt::run(lazy(|| {
    get_server_config()
      .and_then(|response| {
        info!("json: {:#?}", response);
        Ok(())
      })
      .map_err(|err| println!("err: {}", err))
  }));
}

pub fn get_ping_from_server(server: String) -> impl Future<Item = (String, u64), Error = Error> {
  new_client().and_then(move |client| {
    let url = format!("{}/front/0k", server).parse().unwrap();

    let start_time = precise_time_ns();
    client.get(url).then(move |result| match result {
      Ok(_) => Ok((server, precise_time_ns() - start_time)),
      Err(_) => Ok((server, std::u64::MAX)),
    })
  })
}

/// ping every server in `.servers` and sort by nanoseconds ping
pub fn get_servers_sorted_by_ping() -> impl Future<Item = (Vec<String>, Vec<u64>), Error = Error> {
  info!("get_servers_sorted_by_ping");

  get_server_config().and_then(|response| {
    stream::iter_ok(response.servers)
      .and_then(|server| Ok(get_ping_from_server(server)))
      .buffer_unordered(4)
      .collect()
      .and_then(|mut pings| {
        pings.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let servers = pings.iter().map(|a| a.0.to_owned()).collect();
        let server_ping_times = pings.iter().map(|a| a.1).collect();
        Ok((servers, server_ping_times))
      })
  })
}

#[test]
fn test_get_servers_sorted_by_ping() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  hyper::rt::run(lazy(|| {
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        info!("sorted latency: {:#?}", servers);
        Ok(())
      })
      .map_err(|err| println!("err: {}", err))
  }));
}

/// returns a Future<Stream> where the stream will output (bytes, nanoseconds) every `update_interval` milliseconds
pub fn get_download_speed_stream(
  server: String,
  update_interval: u16,
) -> impl Future<Item = impl Stream<Item = (u64, u64), Error = Error>, Error = Error> {
  info!("get_download_speed_stream {}", server);

  new_client().and_then(move |client| {
    client
      .get(format!("{}/front/k", server).parse().unwrap())
      .from_err()
      .and_then(move |response| {
        let body = response.into_body();

        let mut total_bytes = 0;
        let mut start_time = precise_time_ns();

        Ok(body.from_err().filter_map(move |chunk| {
          let now = precise_time_ns();
          let total_nanoseconds = now - start_time;

          total_bytes += chunk.as_ref().len() as u64;

          if total_nanoseconds >= (1_000_000 * u64::from(update_interval)) {
            let a = Some((total_bytes, total_nanoseconds));

            start_time = now;
            total_bytes = 0;

            a
          } else {
            None
          }
        }))
      })
  })
}

#[test]
fn test_get_download_speed_stream() {
  env_logger::Builder::from_default_env()
    .filter(Some("dslreports"), log::LevelFilter::Info)
    .init();

  hyper::rt::run(lazy(|| {
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        stream::iter_ok(servers)
          .take(1)
          .and_then(move |server| {
            Ok(lazy(move || {
              get_download_speed_stream(server.clone(), 1000).and_then(move |stream| {
                stream.for_each(move |(total_bytes, total_nanoseconds)| {
                  let rate = ((total_bytes as f64) / 1_000_000.0)
                    / ((total_nanoseconds as f64) / 1_000_000_000.0);

                  info!("{}", rate);

                  Ok(())
                })
              })
            }))
          })
          .buffer_unordered(1)
          .collect()
      })
      .map(|_| ())
      .map_err(|_| ())
  }));
}
