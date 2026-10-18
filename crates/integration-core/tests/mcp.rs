use mind_place_archive::{ArchiveStore,Capture,Message};
use mind_place_index::IndexStore;
use mind_place_integration::{mcp::{self,dispatch,LEGACY,MODERN},transport::LocalMcpServer,ClientAuthority,IntegrationCore};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability,StateStore};
use serde_json::{json,Value};
use std::io::{Read,Write};

const ID:&str="11111111-2222-4333-8444-555555555555";
struct Fixture { temp:tempfile::TempDir, archive:ArchiveStore,index:IndexStore,state:StateStore,security:SecurityGate,token:String }
impl Fixture {
 fn new()->Self {
  let temp=tempfile::tempdir().unwrap();
  let archive=ArchiveStore::open(temp.path().join("archive-v1")).unwrap();
  let state=StateStore::open(temp.path().join("state.sqlite3")).unwrap();
  let index=IndexStore::open(temp.path().join("index.sqlite3")).unwrap();
  archive.save(Capture { schema_version:1,provider:"chatgpt".into(),conversation_id:ID.into(),
   source_url:format!("https://chatgpt.com/c/{ID}"),title:"Transport".into(),
   conversation_created_at:None,conversation_updated_at:None,captured_at:"2026-10-18T11:00:00Z".into(),
   capture_method:"manual-dom".into(),completeness:"partial".into(),warnings:vec![],
   messages:vec![Message{id:"m1".into(),role:"user".into(),text:"amber eclipse knowledge".into(),
   format:"plain".into(),authored_at:None,model:None,attachments:vec![]}]}).unwrap();
  index.sync(&archive,&state).unwrap();
  let security=SecurityGate::open(temp.path().join("security.json")).unwrap();
  security.initialize(b"test-password-456").unwrap();
  let token=ClientAuthority::new(&state,&security).issue("Claude Code",&[ClientCapability::Read])
   .unwrap().credential().to_string();
  Self{temp,archive,index,state,security,token}
 }
 fn core(&self)->IntegrationCore<'_>{IntegrationCore::new(&self.archive,&self.index,&self.security)}
 fn call(&self,method:&str,params:Value,name:Option<&str>)->mcp::McpReply{
  dispatch(&self.core(),&self.state,&self.token,json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),MODERN,Some(method),name)
 }
}
fn meta()->Value{json!({"io.modelcontextprotocol/protocolVersion":MODERN,"io.modelcontextprotocol/clientCapabilities":{}})}

#[test]
fn modern_wire_has_discovery_tools_and_verified_evidence() {
 let f=Fixture::new();
 let discover=f.call("server/discover",json!({"_meta":meta()}),None);
 assert_eq!(discover.status,200);
 assert_eq!(discover.body.as_ref().unwrap()["result"]["supportedVersions"][0],MODERN);
 let list=f.call("tools/list",json!({"_meta":meta()}),None);
 assert_eq!(list.body.as_ref().unwrap()["result"]["tools"].as_array().unwrap().len(),3);
 let hit=f.call("tools/call",json!({"_meta":meta(),"name":"memory_search","arguments":{"queries":["amber eclipse"]}}),Some("memory_search"));
 assert_eq!(hit.status,200);
 assert_eq!(hit.body.as_ref().unwrap()["result"]["structuredContent"]["hits"][0]["message_id"],"m1");
 assert_eq!(hit.body.as_ref().unwrap()["result"]["resultType"],"complete");
}
#[test]
fn legacy_handshake_and_modern_header_guards() {
 let f=Fixture::new();
 let hello=dispatch(&f.core(),&f.state,&f.token,
  json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":LEGACY,"capabilities":{},"clientInfo":{"name":"codex","version":"1"}}}),LEGACY,None,None);
 assert_eq!(hello.body.as_ref().unwrap()["result"]["protocolVersion"],LEGACY);
 let mismatched=dispatch(&f.core(),&f.state,&f.token,json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
  "params":{"_meta":meta(),"name":"memory_search","arguments":{"queries":["amber"]}}}),MODERN,Some("tools/call"),Some("memory_fetch"));
 assert_eq!(mismatched.body.as_ref().unwrap()["error"]["code"],-32020);
 let forbidden=dispatch(&f.core(),&f.state,"no-token",json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":meta()}}),MODERN,Some("server/discover"),None);
 assert_eq!(forbidden.status,401);
 f.security.lock();
 assert_ne!(f.call("tools/list",json!({"_meta":meta()}),None).status,200);
}
fn tcp_request(payload:String)->String {
 let mut connection=std::net::TcpStream::connect(("127.0.0.1",mcp::MCP_PORT)).unwrap();
 connection.set_read_timeout(Some(std::time::Duration::from_secs(8))).unwrap();
 connection.write_all(payload.as_bytes()).unwrap();
 let mut bytes=Vec::new();
 connection.read_to_end(&mut bytes).unwrap();
 String::from_utf8(bytes).unwrap()
}
fn raw(token:&str,host:&str,origin:&str)->String{
 let data=json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":meta()}}).to_string();
 format!("POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {token}\r\nMCP-Protocol-Version: 2026-07-28\r\nMcp-Method: server/discover\r\n{origin}Content-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len())
}
#[tokio::test]
async fn real_http_deny_rebinding_origin_and_locked_epoch() {
 let f=Fixture::new();
 let server=LocalMcpServer::start(f.archive.clone(),f.security.clone(),f.state.path().to_owned(),f.temp.path().join("index.sqlite3")).await.unwrap();
 let token=f.token.clone();
 let response=tokio::task::spawn_blocking(move||tcp_request(raw(&token,"127.0.0.1:43731",""))).await.unwrap();
 assert!(response.starts_with("HTTP/1.1 200"),"{response}");
 assert!(response.contains("supportedVersions"));
 let token=f.token.clone();
 let denied=tokio::task::spawn_blocking(move||tcp_request(raw(&token,"evil.example",""))).await.unwrap();
 assert!(denied.starts_with("HTTP/1.1 403"));
 let token=f.token.clone();
 let denied=tokio::task::spawn_blocking(move||tcp_request(raw(&token,"127.0.0.1:43731","Origin: http://localhost:3000\r\n"))).await.unwrap();
 assert!(denied.starts_with("HTTP/1.1 403"));
 f.security.lock();
 let token=f.token.clone();
 let denied=tokio::task::spawn_blocking(move||tcp_request(raw(&token,"127.0.0.1:43731",""))).await.unwrap();
 assert!(!denied.contains("supportedVersions"));
 server.stop();
}
