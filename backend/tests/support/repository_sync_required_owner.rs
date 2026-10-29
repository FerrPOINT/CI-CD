//! Used only in the isolated, required-policy invocation of the exact-SHA test.
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use pdlc_shared::space_access::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;

#[derive(Clone)]
struct Decision {
    subject: Uuid,
    denied_subject: Uuid,
    issuer: String,
    allowed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    spaces: Arc<std::sync::RwLock<Vec<SpaceRef>>>,
}
pub(super) struct RequiredOwner {
    pub subject: Uuid,
    pub allowed: Arc<AtomicBool>,
    pub calls: Arc<AtomicUsize>,
    pub spaces: Arc<std::sync::RwLock<Vec<SpaceRef>>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for RequiredOwner {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl RequiredOwner {
    pub async fn new(pool: &sqlx::PgPool) -> Self {
        let subject = Uuid::new_v4();
        let denied_subject = Uuid::new_v4();
        for (id, token) in [
            (subject, "cicd_fixture_sync_owner"),
            (denied_subject, "cicd_fixture_sync_no_spaces"),
        ] {
            let local = Uuid::new_v4();
            sqlx::query("INSERT INTO users(id,username,role,enabled,central_sub) VALUES($1,$2,'admin',true,$3)")
                .bind(local).bind(format!("sync-owner-{local}")).bind(id.to_string()).execute(pool).await.unwrap();
            sqlx::query("INSERT INTO api_tokens(id,name,token_hash,token_hint,user_id,scopes) VALUES($1,'Owned fixture',$2,'fixture',$3,ARRAY['api:read','api:write','git:write','ci-cd:read','ci-cd:write'])")
                .bind(Uuid::new_v4()).bind(cicd::auth::hash_token(token)).bind(local).execute(pool).await.unwrap();
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let allowed = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let spaces = Arc::new(std::sync::RwLock::new(Vec::new()));
        let decision = Decision {
            subject,
            denied_subject,
            issuer: issuer.clone(),
            allowed: allowed.clone(),
            calls: calls.clone(),
            spaces: spaces.clone(),
        };
        let router=Router::new().route("/oidc/jwks",get(||async{Json(json!({"keys":[]}))})).route("/auth/access/decisions",post(|State(state):State<Decision>, headers:HeaderMap, Json(request):Json<Value>|async move {
            assert_eq!(headers["authorization"],"Bearer fixture-sync-owner-reader");
            let subject:Uuid=serde_json::from_value(request["subjects"][0].clone()).unwrap();
            assert!([state.subject,state.denied_subject].contains(&subject));
            assert_eq!(request,json!({"schema_version":1,"subjects":[subject]}));
            state.calls.fetch_add(1,Ordering::SeqCst);
            let now=chrono::Utc::now();
            // Admin grants every application; revocation also removes that role.
            Json(json!({"schema_version":1,"decisions":[{
                "schema_version":1,"issuer":state.issuer,"subject":subject,"role":if state.allowed.load(Ordering::SeqCst){"admin"}else{"user"},
                "applications":if state.allowed.load(Ordering::SeqCst){vec!["ci-cd"]}else{vec![]},
                "revision":1,"enforced":true,"observed_at":now,"expires_at":now+chrono::Duration::seconds(30)
            }]}))
        })).route("/admission",post(|State(state):State<Decision>, headers:HeaderMap, Json(request):Json<SpaceAdmissionRequest>|async move {
            assert_eq!(headers["authorization"],"Bearer fixture-sync-space-reader");
            let subject:Uuid=request.identity.subject.parse().unwrap();
            assert!([state.subject,state.denied_subject].contains(&subject));
            let now=chrono::Utc::now().timestamp();
            Json(SpaceAdmission {
                schema_version:1,identity:request.identity,identity_active:true,policy_revision:1,
                policy:SpaceAccessPolicy::Selected { spaces:if subject==state.subject{state.spaces.read().unwrap().clone()}else{vec![]} },inheritance:None,
                validity:AdmissionValidity{observed_at:now,valid_until:now+30},
                spaces:request.spaces.into_iter().map(|space|ConfirmedSpace{space,revision:1,state:SpaceState::Active}).collect(),
            })
        })).with_state(decision);
        // This mode runs in a dedicated process, before either OnceCell client starts.
        unsafe {
            std::env::set_var("CICD_SPACES__ADMISSION_URL", format!("{issuer}/admission"));
            std::env::set_var("CICD_SPACES__ADMISSION_TOKEN", "fixture-sync-space-reader");
            std::env::set_var("CICD_AUTH__CENTRAL_ISSUER", &issuer);
            std::env::set_var("CICD_AUTH__CENTRAL_JWKS_URI", format!("{issuer}/oidc/jwks"));
            std::env::set_var(
                "CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN",
                "fixture-sync-owner-reader",
            );
        }
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            subject,
            allowed,
            calls,
            spaces,
            server,
        }
    }
}
