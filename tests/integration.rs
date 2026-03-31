use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

static BUILD_MODULE: Once = Once::new();

fn module_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|dir| if dir.is_absolute() { dir } else { manifest_dir.join(dir) })
        .unwrap_or_else(|| manifest_dir.join("target"));
    let extension = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "linux") {
        "so"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        panic!("unsupported target OS for Redis module tests");
    };

    target_dir
        .join("debug")
        .join(format!("libredis_cron.{extension}"))
}

fn ensure_module_built() {
    BUILD_MODULE.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .arg("build")
            .status()
            .expect("failed to run cargo build for integration test setup");
        assert!(status.success(), "cargo build failed during integration test setup");
    });

    assert!(
        module_path().exists(),
        "expected built Redis module at {}",
        module_path().display()
    );
}

fn find_binary(name: &str) -> PathBuf {
    let output = Command::new("which")
        .arg(name)
        .output()
        .unwrap_or_else(|err| panic!("failed to locate {}: {}", name, err));
    assert!(
        output.status.success(),
        "{name} must be installed and available on PATH for integration tests",
        name = name
    );

    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

fn unique_test_dir() -> PathBuf {
    let base = std::env::temp_dir().join("redis_cron_integration");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock drifted before unix epoch")
        .as_nanos();
    let unique = format!(
        "{}_{}",
        std::process::id(),
        nanos
    );
    let dir = base.join(unique);
    fs::create_dir_all(&dir).expect("failed to create integration test temp dir");
    dir
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind an ephemeral test port");
    let port = listener
        .local_addr()
        .expect("failed to inspect ephemeral port")
        .port();
    drop(listener);
    port
}

struct RedisServer {
    child: Child,
    port: u16,
    dir: PathBuf,
    log_path: PathBuf,
}

impl RedisServer {
    fn start() -> Self {
        ensure_module_built();

        let redis_server = find_binary("redis-server");
        let port = free_port();
        let dir = unique_test_dir();
        let log_path = dir.join("redis.log");

        let child = Command::new(redis_server)
            .arg("--bind")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--save")
            .arg("")
            .arg("--appendonly")
            .arg("no")
            .arg("--protected-mode")
            .arg("no")
            .arg("--dir")
            .arg(&dir)
            .arg("--dbfilename")
            .arg("dump.rdb")
            .arg("--logfile")
            .arg(&log_path)
            .arg("--loadmodule")
            .arg(module_path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start redis-server");

        let server = Self {
            child,
            port,
            dir,
            log_path,
        };
        server.wait_until_ready();
        server
    }

    fn redis_cli(&self, args: &[&str], stdin: Option<&[u8]>) -> Output {
        let redis_cli = find_binary("redis-cli");
        let mut command = Command::new(redis_cli);
        command.arg("--raw").arg("-p").arg(self.port.to_string());
        command.args(args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }

        let mut child = command
            .spawn()
            .unwrap_or_else(|err| panic!("failed to spawn redis-cli: {}", err));

        if let Some(stdin_bytes) = stdin {
            use std::io::Write;

            child
                .stdin
                .as_mut()
                .expect("redis-cli stdin pipe missing")
                .write_all(stdin_bytes)
                .expect("failed to write stdin to redis-cli");
        }

        child
            .wait_with_output()
            .unwrap_or_else(|err| panic!("failed to wait on redis-cli: {}", err))
    }

    fn redis_cli_ok(&self, args: &[&str]) -> String {
        self.redis_cli_ok_with_stdin(args, None)
    }

    fn redis_cli_ok_with_stdin(&self, args: &[&str], stdin: Option<&[u8]>) -> String {
        let output = self.redis_cli(args, stdin);
        assert!(
            output.status.success(),
            "redis-cli command failed: {:?}\nstdout: {}\nstderr: {}\nredis log:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            self.read_log()
        );
        String::from_utf8(output.stdout)
            .expect("redis-cli produced non-utf8 stdout")
            .trim()
            .to_owned()
    }

    fn wait_until_ready(&self) {
        self.wait_for(
            || {
                let output = self.redis_cli(&["PING"], None);
                output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "PONG"
            },
            Duration::from_secs(10),
            "redis-server did not become ready in time",
        );
    }

    fn wait_for<F>(&self, mut condition: F, timeout: Duration, message: &str)
    where
        F: FnMut() -> bool,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }

        panic!("{}\nredis log:\n{}", message, self.read_log());
    }

    fn read_log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

impl Drop for RedisServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn parse_i64(s: &str) -> i64 {
    s.trim()
        .parse::<i64>()
        .unwrap_or_else(|err| panic!("failed to parse integer from {:?}: {}", s, err))
}

fn parse_optional_i64(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(parse_i64(trimmed))
    }
}

fn eval_job_count(server: &RedisServer) -> i64 {
    parse_i64(&server.redis_cli_ok(&["EVAL", "return #redis.call('CRON.LIST')", "0"]))
}

fn eval_first_job_id(server: &RedisServer) -> String {
    server.redis_cli_ok(&["EVAL", "return redis.call('CRON.LIST')[1][1]", "0"])
}

fn read_bytes(server: &RedisServer, key: &str) -> Vec<i64> {
    server
        .redis_cli_ok(&[
            "EVAL",
            "local v=redis.call('GET', KEYS[1]); return {string.byte(v, 1, -1)}",
            "1",
            key,
        ])
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_i64)
        .collect()
}

#[test]
fn incr_job_runs_and_can_be_unscheduled() {
    let server = RedisServer::start();
    let key = "cron:test:counter";

    assert_eq!(server.redis_cli_ok(&["DEL", key]), "0");

    let job_id = server.redis_cli_ok(&["CRON.SCHEDULE", "* * * * * *", "INCR", key]);
    assert_eq!(eval_job_count(&server), 1);
    assert_eq!(eval_first_job_id(&server), job_id);

    server.wait_for(
        || parse_optional_i64(&server.redis_cli_ok(&["GET", key])).is_some_and(|value| value >= 2),
        Duration::from_secs(6),
        "scheduled INCR job did not execute twice in time",
    );

    assert_eq!(server.redis_cli_ok(&["CRON.UNSCHEDULE", &job_id]), "1");
    assert_eq!(eval_job_count(&server), 0);
}

#[test]
fn binary_arguments_are_preserved() {
    let server = RedisServer::start();
    let key = "cron:test:bin";
    let payload = b"\x00\xffhello";

    let job_id = server.redis_cli_ok_with_stdin(
        &["-x", "CRON.SCHEDULE", "* * * * * *", "SET", key],
        Some(payload),
    );

    server.wait_for(
        || server.redis_cli_ok(&["EXISTS", key]) == "1",
        Duration::from_secs(6),
        "binary SET job did not create the key in time",
    );

    assert_eq!(server.redis_cli_ok(&["STRLEN", key]), "7");
    assert_eq!(read_bytes(&server, key), vec![0, 255, 104, 101, 108, 108, 111]);

    assert_eq!(server.redis_cli_ok(&["CRON.UNSCHEDULE", &job_id]), "1");
}

#[test]
fn failing_job_does_not_stop_scheduler() {
    let server = RedisServer::start();
    let key = "cron:test:good-counter";

    let bad_job_id = server.redis_cli_ok(&["CRON.SCHEDULE", "* * * * * *", "INCR"]);
    let good_job_id = server.redis_cli_ok(&["CRON.SCHEDULE", "* * * * * *", "INCR", key]);

    server.wait_for(
        || parse_optional_i64(&server.redis_cli_ok(&["GET", key])).is_some_and(|value| value >= 2),
        Duration::from_secs(6),
        "good job stopped running after a scheduled command error",
    );

    assert_eq!(server.redis_cli_ok(&["CRON.UNSCHEDULE", &bad_job_id]), "1");
    assert_eq!(server.redis_cli_ok(&["CRON.UNSCHEDULE", &good_job_id]), "1");
}
