use crate::models::ChatMessage;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
    // One runtime per database. Keeps restart recovery from touching another server's runs.
    _runtime_lock: Arc<tokio::sync::Mutex<sqlx::pool::PoolConnection<sqlx::Postgres>>>,
}
#[derive(Debug)]
pub enum StoreError {
    Unavailable,
    NotFound,
    Conflict,
}
impl From<sqlx::Error> for StoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Serialize, sqlx::FromRow)]
pub struct ConversationSummary {
    pub id: Uuid,
    pub title: String,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize)]
pub struct RunView {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub status: String,
    pub reason: Option<String>,
    pub prompt: String,
    pub response: Option<String>,
    pub error: Option<String>,
    pub activity: String,
    pub activities: Vec<String>,
}
impl Store {
    pub async fn connect(url: &str) -> Result<Self, String> {
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .acquire_timeout(Duration::from_secs(5))
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout = '5s'")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(url)
            .await
            .map_err(|_| "无法连接 Postgres，请检查 DATABASE_URL 与数据库状态")?;
        let mut lock = pool.acquire().await.map_err(|_| "无法获取数据库连接")?;
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(6142279183)")
            .fetch_one(&mut *lock)
            .await
            .map_err(|_| "无法锁定 Agent 实例")?;
        if !locked {
            return Err("此数据库已有一个 Agent 服务在运行".into());
        }
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|_| "数据库迁移失败，请检查迁移状态")?;
        sqlx::query("UPDATE runs SET status='failed', reason='interrupted', activity='运行已中断', error='服务已重启，上次运行中断。请重新发送。', finished_at=now() WHERE status='running'")
            .execute(&pool).await.map_err(|_| "无法恢复上次运行状态")?;
        Ok(Self {
            pool,
            _runtime_lock: Arc::new(tokio::sync::Mutex::new(lock)),
        })
    }
    // A reconnecting pool must not allow an old runtime to write after losing its
    // dedicated advisory-lock connection (for example after a database restart).
    async fn check_runtime(&self) -> Result<(), StoreError> {
        let mut connection = self._runtime_lock.lock().await;
        sqlx::query("SELECT 1").execute(&mut **connection).await?;
        Ok(())
    }
    pub async fn begin(
        &self,
        owner: &str,
        conversation: Uuid,
        id: Uuid,
        prompt: &str,
        snapshot: Value,
        duration: Duration,
    ) -> Result<bool, StoreError> {
        self.check_runtime().await?;
        let mut tx = self.pool.begin().await?;
        let title: String = prompt
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(32)
            .collect();
        sqlx::query("INSERT INTO conversations(id,owner_id,title) VALUES($1,$2,$3) ON CONFLICT(id) DO NOTHING")
            .bind(conversation).bind(owner).bind(title).execute(&mut *tx).await?;
        let actual: String =
            sqlx::query_scalar("SELECT owner_id FROM conversations WHERE id=$1 FOR UPDATE")
                .bind(conversation)
                .fetch_one(&mut *tx)
                .await?;
        if actual != owner {
            return Err(StoreError::NotFound);
        }
        let existing = sqlx::query("SELECT r.conversation_id, e.payload FROM runs r JOIN conversation_entries e ON e.run_id=r.id AND e.kind='user_message' WHERE r.id=$1")
            .bind(id).fetch_optional(&mut *tx).await?;
        if let Some(row) = existing {
            let payload: Value = row.get("payload");
            if row.get::<Uuid, _>("conversation_id") != conversation
                || payload["content"][0]["text"] != prompt
            {
                return Err(StoreError::Conflict);
            }
            tx.commit().await?;
            return Ok(false);
        }
        let busy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE conversation_id=$1 AND status='running')",
        )
        .bind(conversation)
        .fetch_one(&mut *tx)
        .await?;
        if busy {
            return Err(StoreError::Conflict);
        }
        let deadline = Utc::now()
            + chrono::Duration::from_std(duration + Duration::from_secs(15))
                .map_err(|_| StoreError::Unavailable)?;
        sqlx::query("INSERT INTO runs(id,conversation_id,status,snapshot,deadline_at) VALUES($1,$2,'running',$3,$4)")
            .bind(id).bind(conversation).bind(snapshot).bind(deadline).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO conversation_entries(conversation_id,run_id,kind,payload) VALUES($1,$2,'user_message',$3)")
            .bind(conversation).bind(id).bind(message("user",prompt)).execute(&mut *tx).await?;
        sqlx::query("UPDATE conversations SET updated_at=now() WHERE id=$1")
            .bind(conversation)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn append(&self, run: Uuid, kind: &str, payload: Value) -> Result<(), StoreError> {
        self.check_runtime().await?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT conversation_id,status FROM runs WHERE id=$1 FOR UPDATE")
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<String, _>("status") != "running" {
            return Err(StoreError::Conflict);
        }
        sqlx::query("INSERT INTO conversation_entries(conversation_id,run_id,kind,payload) VALUES($1,$2,$3,$4)")
            .bind(row.get::<Uuid,_>("conversation_id")).bind(run).bind(kind).bind(&payload).execute(&mut *tx).await?;
        if kind == "activity" {
            sqlx::query("UPDATE runs SET activity=$2 WHERE id=$1")
                .bind(run)
                .bind(payload["label"].as_str().unwrap_or("处理中"))
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn activity(&self, run: Uuid, label: &str, step: usize) -> Result<(), StoreError> {
        self.append(
            run,
            "activity",
            json!({"schema_version":1,"label":label,"step":step}),
        )
        .await
    }
    pub async fn finish(
        &self,
        run: Uuid,
        reason: &str,
        text: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        self.check_runtime().await?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT conversation_id,status FROM runs WHERE id=$1 FOR UPDATE")
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<String, _>("status") != "running" {
            return Ok(());
        }
        let conversation: Uuid = row.get("conversation_id");
        if let Some(text) = text {
            sqlx::query("INSERT INTO conversation_entries(conversation_id,run_id,kind,payload) VALUES($1,$2,'assistant_message',$3)")
                .bind(conversation).bind(run).bind(message("assistant",text)).execute(&mut *tx).await?;
        }
        let status = if text.is_some() {
            "completed"
        } else {
            "failed"
        };
        sqlx::query("UPDATE runs SET status=$2,reason=$3,error=$4,activity=$5,finished_at=now() WHERE id=$1")
            .bind(run).bind(status).bind(reason).bind(error).bind(if reason == "clarification" {"等待澄清"} else if text.is_some() {"已完成"} else {"运行已停止"}).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO conversation_entries(conversation_id,run_id,kind,payload) VALUES($1,$2,'run_finished',$3)")
            .bind(conversation).bind(run).bind(json!({"schema_version":1,"reason":reason,"status":status})).execute(&mut *tx).await?;
        sqlx::query("UPDATE conversations SET updated_at=now() WHERE id=$1")
            .bind(conversation)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    /// Fail closed after a crashed task or database outage; never replay tools automatically.
    pub async fn expire(&self) -> Result<(), StoreError> {
        sqlx::query("UPDATE runs SET status='failed',reason='interrupted',activity='运行已中断',error='运行状态中断，请重新发送。',finished_at=now() WHERE status='running' AND deadline_at < now()")
            .execute(&self.pool).await?;
        Ok(())
    }
    pub async fn list(
        &self,
        owner: &str,
        offset: i64,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        Ok(sqlx::query_as("SELECT id,title,updated_at FROM conversations WHERE owner_id=$1 ORDER BY updated_at DESC,id LIMIT 100 OFFSET $2")
            .bind(owner).bind(offset).fetch_all(&self.pool).await?)
    }
    pub async fn conversation(
        &self,
        owner: &str,
        id: Uuid,
        offset: i64,
    ) -> Result<Vec<RunView>, StoreError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=$1 AND owner_id=$2)",
        )
        .bind(id)
        .bind(owner)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM runs WHERE conversation_id=$1 ORDER BY created_at DESC,id DESC LIMIT 50 OFFSET $2")
            .bind(id).bind(offset).fetch_all(&self.pool).await?;
        let mut runs = Vec::new();
        for id in ids.into_iter().rev() {
            runs.push(self.view(owner, id).await?);
        }
        Ok(runs)
    }
    pub async fn view(&self, owner: &str, id: Uuid) -> Result<RunView, StoreError> {
        self.expire().await?;
        let row = sqlx::query("SELECT r.* FROM runs r JOIN conversations c ON c.id=r.conversation_id WHERE r.id=$1 AND c.owner_id=$2")
            .bind(id).bind(owner).fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)?;
        let entries = sqlx::query("SELECT kind,payload FROM conversation_entries WHERE run_id=$1 AND kind IN ('user_message','assistant_message','activity') ORDER BY sequence")
            .bind(id).fetch_all(&self.pool).await?;
        let mut prompt = String::new();
        let mut response = None;
        let mut activities = Vec::new();
        for entry in entries {
            let payload: Value = entry.get("payload");
            match entry.get::<&str, _>("kind") {
                "user_message" => {
                    prompt = payload["content"][0]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .into()
                }
                "assistant_message" => {
                    response = Some(
                        payload["content"][0]["text"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                    )
                }
                "activity" => activities.push(payload["label"].as_str().unwrap_or_default().into()),
                _ => {}
            }
        }
        Ok(RunView {
            id,
            conversation_id: row.get("conversation_id"),
            status: row.get("status"),
            reason: row.get("reason"),
            error: row.get("error"),
            activity: row.get("activity"),
            prompt,
            response,
            activities,
        })
    }
    pub async fn history(
        &self,
        conversation: Uuid,
        current: Uuid,
        turns: usize,
        max_chars: usize,
    ) -> Result<Vec<ChatMessage>, StoreError> {
        // Preserve the whole unresolved question across clarification turns, even
        // when history_turns is small. The Agent's context limit still applies;
        // never silently drop conditions and guess an answer.
        let pending: Option<Value> = sqlx::query_scalar("SELECT e.payload FROM conversation_entries e JOIN runs r ON r.id=e.run_id WHERE e.kind='clarification' AND r.reason='clarification' AND r.id=(SELECT id FROM runs WHERE conversation_id=$1 AND status='completed' AND id<>$2 AND created_at < (SELECT created_at FROM runs WHERE id=$2) ORDER BY created_at DESC LIMIT 1) ORDER BY e.sequence DESC LIMIT 1")
            .bind(conversation).bind(current).fetch_optional(&self.pool).await?;
        if let Some(pending) = pending {
            let mut messages: Vec<ChatMessage> =
                serde_json::from_value(pending["messages"].clone())
                    .map_err(|_| StoreError::Unavailable)?;
            messages.push(ChatMessage {
                role: "assistant".into(),
                text: pending["question"]["text"]
                    .as_str()
                    .ok_or(StoreError::Unavailable)?
                    .into(),
            });
            let payload: Value = sqlx::query_scalar("SELECT payload FROM conversation_entries WHERE run_id=$1 AND conversation_id=$2 AND kind='user_message' ORDER BY sequence LIMIT 1")
                .bind(current).bind(conversation).fetch_one(&self.pool).await?;
            messages.push(ChatMessage {
                role: "user".into(),
                text: payload["content"][0]["text"]
                    .as_str()
                    .ok_or(StoreError::Unavailable)?
                    .into(),
            });
            return Ok(messages);
        }
        let rows = sqlx::query("SELECT e.kind,e.payload,e.run_id FROM conversation_entries e WHERE e.conversation_id=$1 AND e.kind IN ('user_message','assistant_message') AND e.run_id IN (SELECT id FROM runs WHERE conversation_id=$1 AND (status='completed' OR id=$2) ORDER BY created_at DESC LIMIT $3) ORDER BY e.sequence")
            .bind(conversation).bind(current).bind(turns as i64).fetch_all(&self.pool).await?;
        let mut groups: Vec<(Uuid, Vec<ChatMessage>)> = Vec::new();
        for row in rows {
            let id: Uuid = row.get("run_id");
            let payload: Value = row.get("payload");
            let msg = ChatMessage {
                role: payload["role"].as_str().unwrap_or("user").into(),
                text: payload["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
            };
            if groups.last().is_none_or(|g| g.0 != id) {
                groups.push((id, Vec::new()));
            }
            groups.last_mut().unwrap().1.push(msg);
        }
        let mut count = 0;
        let mut selected = Vec::new();
        for (_, messages) in groups.into_iter().rev() {
            let size: usize = messages.iter().map(|m| m.text.chars().count()).sum();
            if count + size > max_chars {
                break;
            }
            count += size;
            selected.push(messages);
        }
        Ok(selected.into_iter().rev().flatten().collect())
    }
}
fn message(role: &str, text: &str) -> Value {
    json!({"schema_version":1,"role":role,"content":[{"type":"text","text":text}]})
}
