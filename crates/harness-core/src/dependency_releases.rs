//! Explicit read-only release planning; default inventory has no network path.
#![cfg(windows)]
use crate::{dependency_discovery, dependency_fetch, dependency_package, dependency_plan};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};

pub fn plan(request: &dependency_discovery::Request) -> io::Result<Value> {
    let catalogue_path = dependency_discovery::local_path(&request.catalogue)?;
    let catalogue =
        dependency_package::read_json(&catalogue_path)?.ok_or_else(dependency_package::invalid)?;
    // Validate and use the same parsed snapshot for observations and metadata
    // selection. A concurrent catalogue edit cannot replace half the input.
    let inventory = dependency_discovery::discover_with_catalogue(request, catalogue.clone())?;
    let specs: Vec<_> = ["mcp", "languages"]
        .into_iter()
        .flat_map(|group| catalogue[group].as_array().unwrap().iter())
        .collect();
    let client = dependency_fetch::Client::new();
    let mut releases = BTreeMap::new();
    // At most two clients execute concurrently. Each has its own bounded job,
    // 16 MiB body, 50-second deadline and private cleanup scope.
    for batch in specs.chunks(2) {
        std::thread::scope(|scope| -> io::Result<()> {
            let handles: Vec<_> = batch
                .iter()
                .map(|spec| {
                    let client = &client;
                    scope.spawn(move || {
                        let fetched = match client {
                            Ok(client) => client.fetch(spec),
                            Err(reason) => Err(*reason),
                        };
                        let result = dependency_plan::release(
                            spec,
                            fetched.as_deref().map_err(|reason| *reason),
                        );
                        (spec["id"].as_str().unwrap().to_owned(), result)
                    })
                })
                .collect();
            for handle in handles {
                let (id, result) = handle
                    .join()
                    .map_err(|_| io::Error::other("dependency metadata worker failed"))?;
                releases.insert(id, result);
            }
            Ok(())
        })?;
    }
    let mut result = dependency_plan::plan(&catalogue, &inventory, &releases)?;
    result["model_calls"] = json!(0);
    result["processes_started"] = Value::Null;
    result["network_requested"] = json!(true);
    result["metadata_transport"] = match client {
        Ok(client) => {
            json!({"client":"Windows system curl", "version":client.version,"concurrency":2,"body_limit_bytes":16*1024*1024,"transfer_deadline_seconds":50,"redirects":"rejected"})
        }
        Err(reason) => json!({"client":"unavailable", "reason":reason}),
    };
    result["observation"] = json!({"catalogue":inventory["catalogue"], "user_home":inventory["user_home"], "version_probes_requested":inventory["version_probes_requested"], "process_inspection_requested":inventory["process_inspection_requested"]});
    Ok(result)
}
