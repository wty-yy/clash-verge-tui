use serde_json::{json, Value};
pub fn response(path: &str) -> Value {
    match path {
        "/version" => json!({"version":"test-mihomo"}),
        "/configs" => {
            json!({"mode":"rule","mixed-port":17897,"log-level":"info","ipv6":false,"secret":"must-not-display","authentication":["private"]})
        }
        "/proxies" => {
            json!({"proxies":{"Main / #?":{"type":"Selector","all":["First","Second"],"now":"First"},"AUTO":{"type":"URLTest","all":["First"],"now":"First"},"First":{"type":"VLESS","history":[{"delay":12}]},"Second":{"type":"Trojan","history":[]},"Unrelated":{"type":"DIRECT"}}})
        }
        "/rules" => {
            json!({"rules":[{"index":7,"type":"DOMAIN","payload":"example.com","proxy":"Main / #?","extra":{"disabled":false}}]})
        }
        "/providers/rules" => {
            json!({"providers":{"rules / #":{"behavior":"Domain","vehicleType":"HTTP","ruleCount":3,"updatedAt":"now"}}})
        }
        "/connections" => {
            json!({"downloadTotal":1024,"uploadTotal":128,"memory":4096,"connections":[{"id":"uuid-connection","metadata":{"host":"example.com","destinationPort":"443","network":"tcp","process":"fixture"},"chains":["First"],"rule":"DOMAIN","download":1024,"upload":128}]})
        }
        _ => json!({}),
    }
}
