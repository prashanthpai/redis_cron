#[macro_use]
extern crate redis_module;

use lazy_static::lazy_static;
use redis_module::{Context, RedisError, RedisResult, RedisValue, Status};
use std::sync::Mutex;
use std::{thread, time};

mod job_scheduler;
use crate::job_scheduler::{Job, JobScheduler, Uuid};

const SCHED_SLEEP_MS: u64 = 500;

lazy_static! {
    static ref SCHED: Mutex<JobScheduler> = Mutex::new(JobScheduler::new());
}

fn cron_schedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() < 3 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let job_id = SCHED
        .lock()
        .unwrap()
        .add(Job::new(args[1].parse()?, args[2..].to_vec()))
        .to_string();

    return Ok(job_id.into());
}

fn cron_unschedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() != 2 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let job_id = match Uuid::parse_str(&args[1]) {
        Ok(v) => v,
        // return 0 if UUID is invalid
        Err(_err) => return Ok(RedisValue::Integer(false.into())),
    };

    let present = SCHED.lock().unwrap().remove(job_id);

    return Ok(RedisValue::Integer(present.into()));
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
