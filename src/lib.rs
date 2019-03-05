use futures::stream;
use hyper::{
  self,
  rt::{Future, Stream},
  Client,
};
use hyper_tls::HttpsConnector;
use log::*;
use serde::Deserialize;
use serde_json::*;
use time::precise_time_ns;

#[derive(Debug, Deserialize)]
pub struct DSLReportsResponse {
  locations: Vec<String>,
  plat: String,
  server_rids: Vec<String>,
  locations_short: Vec<String>,
  cc_ip2: String,
  key: String,
  cc_ubuntu: String,
  ports: Vec<String>,
  prefs: DSLReportsResponsePrefs,
  dnsdom: Option<String>,
  servers: Vec<String>,
  ipaddr: String,
}

#[derive(Debug, Deserialize)]
pub struct DSLReportsResponsePrefs {
  ipv6: u8,
  https: u8,
}

pub fn get_server_config() -> impl Future<Item = DSLReportsResponse, Error = hyper::Error> {
  let client = Client::builder().build::<_, hyper::Body>(HttpsConnector::new(4).unwrap());

  info!("get_server_config");
  client
    .get(
      "https://api.dslreports.com/speedtest/1.0/?typ=p&plat=10&apikey=12345678"
        .parse()
        .unwrap(),
    )
    .and_then(|response| {
      response.into_body().concat2().and_then(|body| {
        let parsed: DSLReportsResponse = from_reader(body.as_ref()).unwrap();

        Ok(parsed)
      })
    })
}

/// ping every server in `.servers` and sort by nanoseconds ping
pub fn get_servers_sorted_by_ping(
) -> impl Future<Item = (Vec<String>, Vec<u64>), Error = hyper::Error> {
  info!("get_servers_sorted_by_ping");

  get_server_config().and_then(|response| {
    stream::iter_ok(response.servers)
      .and_then(|server| {
        Ok(futures::lazy(|| {
          let client = Client::builder().build::<_, hyper::Body>(HttpsConnector::new(4).unwrap());

          let start_time = precise_time_ns();

          client
            .get(format!("{}/front/0k", server.clone()).parse().unwrap())
            .then(move |result| match result {
              Ok(_) => Ok((server, precise_time_ns() - start_time)),
              Err(_) => Ok((server, !0)),
            })
        }))
      })
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

pub fn get_download_speed_stream(
  server: String,
) -> impl Future<Item = impl Stream<Item = (u64, u64), Error = hyper::Error>, Error = hyper::Error>
{
  let client = Client::builder().build::<_, hyper::Body>(HttpsConnector::new(4).unwrap());

  info!("get_download_speed_stream {}", server);
  client
    .get(format!("{}/front/k", server).parse().unwrap())
    .and_then(|response| {
      let mut total_bytes = 0;
      let mut start_time = precise_time_ns();

      Ok(response.into_body().filter_map(move |chunk| {
        let now = precise_time_ns();
        let total_nanoseconds = now - start_time;

        total_bytes += chunk.as_ref().len() as u64;

        if total_nanoseconds > 1_000_000_000 {
          let a = Some((total_bytes, total_nanoseconds));

          start_time = now;
          total_bytes = 0;

          a
        } else {
          None
        }
      }))
    })
}

#[test]
fn test_asdf() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();
  use hyper::rt;

  rt::run(rt::lazy(|| {
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        let server = servers[0].to_owned();
        get_download_speed_stream(server).and_then(|stream| {
          stream.for_each(|(total_bytes, total_nanoseconds)| {
            let rate =
              ((total_bytes as f64) / 1_000_000.0) / ((total_nanoseconds as f64) / 1_000_000_000.0);

            println!("rate: {}", rate);

            Ok(())
          })
        })
      })
      .map(|_| ())
      .map_err(|_| ())
  }));
}
