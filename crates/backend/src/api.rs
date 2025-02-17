use std::{collections::HashSet, fmt::Display};

use actix_web::{web, HttpResponse, Scope};
use apalis_core::{storage::Storage, task::task_id::TaskId};
use serde::{de::DeserializeOwned, Serialize};
use shared::{BackendExt, Filter, GetJobsResult};
use tokio::sync::RwLock;

pub struct ApiBuilder {
    scope: Scope,
    list: HashSet<String>,
}

impl ApiBuilder {
    pub fn add_storage<J, S>(mut self, storage: &S, namespace: &str) -> Self
    where
        J: Serialize + DeserializeOwned + 'static,
        S: BackendExt<J> + Clone,
        S: Storage<Job = J>,
        S: 'static + Send,
        S::Context: Serialize,
        S::Request: Serialize,
        <S as Storage>::Error: Display,
    {
        self.list.insert(namespace.to_string());

        Self {
            scope: self.scope.service(
                Scope::new(namespace)
                    .app_data(web::Data::new(RwLock::new(storage.clone())))
                    .route("", web::get().to(get_jobs::<J, S>)) // Fetch jobs in queue
                    .route("/workers", web::get().to(get_workers::<J, S>)) // Fetch jobs in queue
                    .route("/job", web::put().to(push_job::<J, S>)) // Allow add jobs via api
                    .route("/job/{job_id}", web::get().to(get_job::<J, S>)), // Allow fetch specific job
            ),
            list: self.list,
        }
    }

    pub fn build(self) -> Scope {
        async fn fetch_queues(queues: web::Data<HashSet<String>>) -> HttpResponse {
            HttpResponse::Ok().json(queues)
        }

        self.scope
            .app_data(web::Data::new(self.list))
            .route("", web::get().to(fetch_queues))
    }

    pub fn new() -> Self {
        Self {
            scope: Scope::new("backend"),
            list: HashSet::new(),
        }
    }
}

impl Default for ApiBuilder {
    fn default() -> Self {
        Self::new()
    }
}

async fn push_job<J, S>(job: web::Json<J>, storage: web::Data<RwLock<S>>) -> HttpResponse
where
    J: Serialize + DeserializeOwned + 'static,
    S: Storage<Job = J> + Clone,
    S::Error: Display,
{
    let res = storage.write().await.push(job.into_inner()).await;
    match res {
        Ok(parts) => {
            HttpResponse::Ok().body(format!("Job with ID [{}] added to queue", parts.task_id))
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("{e}")),
    }
}

async fn get_jobs<J, S>(storage: web::Data<RwLock<S>>, filter: web::Query<Filter>) -> HttpResponse
where
    J: Serialize + DeserializeOwned + 'static,
    S: Storage<Job = J> + BackendExt<J> + Send,
    S::Request: Serialize,
{
    dbg!(&filter);
    // TODO: fix unwrap
    let stats = storage.read().await.stats().await.unwrap_or_default();
    let res = storage
        .read()
        .await
        .list_jobs(&filter.status, filter.page)
        .await;
    match res {
        Ok(jobs) => HttpResponse::Ok().json(GetJobsResult { stats, jobs }),
        Err(_) => HttpResponse::InternalServerError().json("get_jobs_failed"), //TODO
    }
}

async fn get_workers<J, S>(storage: web::Data<RwLock<S>>) -> HttpResponse
where
    J: Serialize + DeserializeOwned + 'static,
    S: Storage<Job = J> + BackendExt<J> + Clone,
{
    let workers = storage.read().await.list_workers().await;
    match workers {
        Ok(workers) => HttpResponse::Ok().json(workers),
        Err(_) => HttpResponse::InternalServerError().body("get_workers_failed"), //TODO
    }
}

async fn get_job<J, S>(job_id: web::Path<TaskId>, storage: web::Data<RwLock<S>>) -> HttpResponse
where
    J: Serialize + DeserializeOwned + 'static,
    S: Storage<Job = J> + 'static,
    S::Error: Display,
    S::Context: Serialize,
{
    let res = storage.write().await.fetch_by_id(&job_id).await;
    match res {
        Ok(Some(job)) => HttpResponse::Ok().json(job),
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(e) => HttpResponse::InternalServerError().body(format!("{e}")),
    }
}
