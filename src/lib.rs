use actix_web::{client, Error, HttpMessage};
use futures::{stream, Future, Stream};
use log::*;
use serde::Deserialize;
use time::precise_time_ns;

const API_KEY: &str = "12345678"; // test key

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

pub fn get_server_config() -> impl Future<Item = DSLReportsResponse, Error = Error> {
  info!("get_server_config");
  client::get(&format!(
    "https://api.dslreports.com/speedtest/1.0/?typ=p&plat=10&apikey={}",
    API_KEY
  ))
  .finish()
  .unwrap()
  .send()
  .map_err(Error::from)
  .and_then(|response| response.json().from_err().and_then(Ok))
}

#[test]
fn test_get_server_config() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  use actix_web::actix;
  let mut sys = actix::System::new("test_get_server_config");

  sys
    .block_on(get_server_config().and_then(|response| {
      info!("json: {:#?}", response);
      Ok(())
    }))
    .unwrap();
}

/// ping every server in `.servers` and sort by nanoseconds ping
pub fn get_servers_sorted_by_ping() -> impl Future<Item = (Vec<String>, Vec<u64>), Error = Error> {
  info!("get_servers_sorted_by_ping");

  get_server_config().and_then(|response| {
    stream::iter_ok(response.servers)
      .and_then(|server| {
        Ok(futures::lazy(|| {
          let start_time = precise_time_ns();
          client::get(&format!("{}/front/0k", server.clone()))
            .finish()
            .unwrap()
            .send()
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

#[test]
fn test_get_servers_sorted_by_ping() {
  env_logger::Builder::from_default_env()
    .filter(None, log::LevelFilter::Info)
    .init();

  use actix_web::actix;
  let mut sys = actix::System::new("test_get_servers_sorted_by_ping");

  sys
    .block_on(get_servers_sorted_by_ping().and_then(|(servers, _pings)| {
      info!("sorted latency: {:#?}", servers);
      Ok(())
    }))
    .unwrap();
}

/// returns a Future<Stream> where the stream will output (bytes, time) every `update_interval` milliseconds
pub fn get_download_speed_stream(
  server: String,
  update_interval: u16,
) -> impl Future<Item = impl Stream<Item = (u64, u64), Error = Error>, Error = Error> {
  info!("get_download_speed_stream {}", server);
  client::get(&format!("{}/front/k", server))
    .finish()
    .unwrap()
    .send()
    .timeout(std::time::Duration::from_secs(60)) // TODO
    .map_err(Error::from)
    .and_then(move |response| {
      let mut total_bytes = 0;
      let mut start_time = precise_time_ns();

      Ok(response.payload().from_err().filter_map(move |chunk| {
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
}

#[test]
fn test_get_download_speed_stream() {
  env_logger::Builder::from_default_env()
    .filter(Some("dslreports"), log::LevelFilter::Info)
    .init();

  use actix_web::actix;
  let sys = actix::System::new("test_get_download_speed_stream");

  actix::spawn(
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        stream::iter_ok(servers)
          .take(1)
          .and_then(move |server| {
            Ok(futures::lazy(move || {
              get_download_speed_stream(server.clone(), 1000).and_then(move |stream| {
                stream.for_each(move |(total_bytes, total_nanoseconds)| {
                  let rate = ((total_bytes as f64) / 1_000_000.0)
                    / ((total_nanoseconds as f64) / 1_000_000_000.0);

                  info!("{}", rate);

                  actix::System::current().stop();

                  Ok(())
                })
              })
            }))
          })
          .buffer_unordered(1)
          .collect()
      })
      .map(|_| ())
      .map_err(|_| ()),
  );

  sys.run();
}
