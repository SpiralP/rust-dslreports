use dslreports::*;
use futures::prelude::*;
use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::Duration,
};

#[tokio::main]
async fn main() {
  let servers = get_servers_sorted_by_ping().await.unwrap();
  let map: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));

  // take 4 best servers
  for (server, _ping) in &servers[..4] {
    let server = server.clone();
    let map = map.clone();

    {
      let mut map = map.lock().unwrap();
      map.insert(server.clone(), 0.0);
    }

    tokio::spawn(async move {
      let mut stream = get_download_speed_stream(server.clone(), Duration::from_millis(500))
        .await
        .unwrap();

      while let Some(result) = stream.next().await {
        let (bytes, duration) = result.unwrap();

        let rate = ((bytes as f64) / 1_000_000.0) / duration.as_secs_f64();

        let mut map = map.lock().unwrap();
        map.insert(server.clone(), rate);
      }

      let mut map = map.lock().unwrap();
      map.remove(&server).unwrap();
    });
  }

  loop {
    tokio::time::delay_for(Duration::from_millis(250)).await;

    let map = map.lock().unwrap();

    if map.is_empty() {
      break;
    }

    let mut total = 0.0;
    for n in map.values() {
      total += n;
    }

    println!("Mbps: {:.1}", total * 8.0);
  }
}
