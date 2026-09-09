//! Read-only Herdr viewport signal. No image, provider, input or render knowledge.
use crate::herdr_api::{self, ApiConnector};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub(crate) struct Watch {
    stopped: Arc<AtomicBool>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

pub(crate) fn watch(
    connector: Arc<dyn ApiConnector>,
    pane_id: String,
    mut publish: impl FnMut(Result<bool, String>) + Send + 'static,
) -> Result<Watch, String> {
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    std::thread::Builder::new().name(format!("pane-viewport-{pane_id}")).spawn(move || {
        let run = || -> Result<(), String> {
            let subscription = herdr_api::subscribe_params(connector.as_ref(),
                crate::wire::viewport_subscription_params(&pane_id)?, Duration::from_secs(3)).map_err(|e| e.to_string())?;
            subscription.read_timeout(Duration::from_millis(500)).map_err(|e| e.to_string())?;
            // Subscribe before the snapshot so changes during the read remain queued.
            let initial = herdr_api::request_with_connector(connector.as_ref(), "pane.get",
                crate::wire::viewport_target_params(&pane_id)?, Duration::from_secs(3)).map_err(|e| e.to_string())?;
            let mut previous = crate::wire::viewport_response(initial, &pane_id)?;
            if stop.load(Ordering::Acquire) { return Ok(()); }
            publish(Ok(previous));
            let (mut reader, _shutdown) = subscription.into_parts();
            let mut line = Vec::new();
            while !stop.load(Ordering::Acquire) {
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => return Err("Terminal viewport subscription ended.".to_owned()),
                    Ok(_) if line.len() > 64 * 1024 => return Err("Terminal viewport event exceeds its size limit.".to_owned()),
                    Ok(_) => {
                        let next = crate::wire::viewport_event(std::str::from_utf8(&line).map_err(|e| e.to_string())?, &pane_id)?;
                        line.clear();
                        if next != previous {
                            previous = next;
                            publish(Ok(next));
                        }
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                    Err(e) => return Err(format!("Terminal viewport stream failed: {e}")),
                }
            }
            Ok(())
        };
        let mut run = run;
        if let Err(error) = run() { if !stop.load(Ordering::Acquire) { publish(Err(error)); } }
    }).map_err(|e| format!("Terminal viewport observer could not start: {e}"))?;
    Ok(Watch { stopped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        sync::mpsc,
    };

    #[test]
    fn observer_reports_only_bottom_transitions_and_stream_failure() {
        let path = std::env::temp_dir().join(format!(
            "viewport-{}-{}.sock",
            std::process::id(),
            crate::live::monotonic_ns()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let (finish, done) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut events, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(events.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert!(request.contains("pane.scroll_changed") && request.contains("w1:p1"));
            writeln!(events, "{}", json!({"id":"herdr-core:events.subscribe","result":{"type":"subscription_started","host":{"host_id":"fixture","session_id":"fixture"},"sequence":0,"oldest_available_sequence":0}})).unwrap();
            let (mut get, _) = listener.accept().unwrap();
            request.clear();
            BufReader::new(get.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            writeln!(get, "{}", json!({"id":"herdr-core:pane.get","result":{"type":"pane_info","pane":{
                "pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","focused":true,"agent_status":"idle","revision":1,
                "surface":{"kind":"terminal","attach":{"host":{"host_id":"fixture","session_id":"fixture"},"protocol":21,"terminal_id":"fixture","transport":"herdr_client"}},
                "scroll":{"offset_from_bottom":0,"max_offset_from_bottom":10,"viewport_rows":20}
            }}})).unwrap();
            for offset in (1..=100).chain([0]) {
                writeln!(events, "{}", json!({"event":"pane.scroll_changed","data":{"pane_id":"w1:p1","workspace_id":"w1","scroll":{"offset_from_bottom":offset,"max_offset_from_bottom":100,"viewport_rows":20}}})).unwrap();
            }
            done.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        let (send, receive) = mpsc::channel();
        let watch = watch(
            Arc::new(herdr_api::UnixSocketConnector::new(&path)),
            "w1:p1".to_owned(),
            move |v| {
                send.send(v).unwrap();
            },
        )
        .unwrap();
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(3)).unwrap(),
            Ok(true)
        );
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(3)).unwrap(),
            Ok(false)
        );
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(3)).unwrap(),
            Ok(true)
        );
        assert!(
            receive.try_recv().is_err(),
            "offset and output changes while away publish no state transition"
        );
        finish.send(()).unwrap();
        assert!(
            receive
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap_err()
                .contains("ended")
        );
        drop(watch);
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
