#[macro_use]
extern crate redis_module;

use lazy_static::lazy_static;
use redis_module::{Context, RedisError, RedisResult, Status};
use std::sync::Mutex;
use std::{thread, time};

mod job_scheduler;
use crate::job_scheduler::{Job, JobScheduler, Uuid};

const CRON_JOB_EXPR_KEY: &str = "redis_cron::jobid_expr";
const CRON_JOB_CMD_KEY: &str = "redis_cron::jobid_cmd";
const SCHED_SLEEP_MS: u64 = 500;

lazy_static! {
    static ref SCHED: Mutex<JobScheduler> = Mutex::new(JobScheduler::new());
}

fn cron_schedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() < 3 {
        return Err(RedisError::WrongArity);
    }

    let cron_expr = args[1].clone();
    let cron_cmd = args[2..].join(" ");

    let job_id = SCHED
        .lock()
        .unwrap()
        .add(Job::new(cron_expr.parse().unwrap(), args))
        .to_string();

    let expr_key = ctx.open_key_writable(CRON_JOB_EXPR_KEY);
    expr_key.hash_set(&job_id, ctx.create_string(&cron_expr));

    let cmd_key = ctx.open_key_writable(CRON_JOB_CMD_KEY);
    cmd_key.hash_set(&job_id, ctx.create_string(&cron_cmd));

    return Ok(job_id.into());
}

fn cron_unschedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() != 2 {
        return Err(RedisError::WrongArity);
    }

    let expr_key = ctx.open_key_writable(CRON_JOB_EXPR_KEY);
    expr_key.hash_del(&args[1]);

    let cmd_key = ctx.open_key_writable(CRON_JOB_CMD_KEY);
    cmd_key.hash_del(&args[1]);

    SCHED.lock().unwrap().remove(Uuid::parse_str(&args[1])?);

    return Ok(().into());
}

fn init(_: &Context, _: &Vec<String>) -> Status {
    // TODO:
    // at startup, read from stored hashsets and add to scheduler
    // but it requires some parsing

    thread::spawn(move || loop {
        SCHED.lock().unwrap().tick();
        thread::sleep(time::Duration::from_millis(SCHED_SLEEP_MS));
    });

    Status::Ok
}

fn deinit(_: &Context) -> Status {
    // TODO:
    // gracefully stop the thread started in init()

    Status::Ok
}

redis_module! {
    name: "cron",
    version: 1,
    data_types: [],
    init: init,
    deinit: deinit,
    commands: [
        ["cron.schedule", cron_schedule, "write deny-oom", 0, 0, 0],
        ["cron.unschedule", cron_unschedule, "write deny-oom", 0, 0, 0],
    ],
}
