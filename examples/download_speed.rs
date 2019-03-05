use dslreports::*;
use futures::{stream, Future, Stream};

fn main() {
  use actix_web::actix;
  let sys = actix::System::new("test_get_download_speed_stream");

  actix::spawn(
    get_servers_sorted_by_ping()
      .and_then(|(servers, _pings)| {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        let map: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));

        stream::iter_ok(servers)
          .take(4)
          .and_then(move |server| {
            let map = map.clone();
            Ok(futures::lazy(move || {
              get_download_speed_stream(server.clone()).and_then(move |stream| {
                stream.for_each(move |(total_bytes, total_nanoseconds)| {
                  let rate = ((total_bytes as f64) / 1_000_000.0)
                    / ((total_nanoseconds as f64) / 1_000_000_000.0);

                  let total = {
                    let mut map = map.lock().unwrap();
                    map.insert(server.clone(), rate);

                    let mut total = 0.0;
                    for n in map.values() {
                      total += n;
                    }
                    total
                  };

                  println!("total rate: {}", total);

                  Ok(())
                })
              })
            }))
          })
          .buffer_unordered(4)
          .collect()
      })
      .map(|_| ())
      .map_err(|_| ()),
  );

  sys.run();
}
