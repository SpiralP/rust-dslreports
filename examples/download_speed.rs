use dslreports::*;
use futures::{stream, Future, Stream};
use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
};

fn main() {
  let sys = actix::System::new("test_get_download_speed_stream");

  actix::spawn(
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        let map: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));

        stream::iter_ok(servers)
          .take(4) // take 4 best servers
          .and_then(move |server| {
            let map = map.clone();
            Ok(futures::lazy(move || {
              get_download_speed_stream(server.clone(), 100).and_then(move |stream| {
                stream.for_each(move |(num_bytes, nanoseconds)| {
                  // MB/s
                  let rate =
                    ((num_bytes as f64) / 1_000_000.0) / ((nanoseconds as f64) / 1_000_000_000.0);

                  let total = {
                    let mut map = map.lock().unwrap();
                    map.insert(server.clone(), rate);

                    let mut total = 0.0;
                    for n in map.values() {
                      total += n;
                    }
                    total
                  };

                  println!("total Mbps: {:.1}", total * 8.0);

                  Ok(())
                })
              })
            }))
          })
          .buffer_unordered(4) // run 4 concurrent futures
          .collect()
      })
      .map(|_| ())
      .map_err(|_| ()),
  );

  sys.run().unwrap();
}
