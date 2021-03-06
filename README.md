# redis_cron

redis_cron is a simple cron expression based job scheduler for Redis that runs
inside redis as a module. It uses similar syntax as a regular cron and allows
you to schedule redis commands directly on redis. This project is inspired from
PostgreSQL extension [pg_cron](https://github.com/citusdata/pg_cron).

redis_cron runs scheduled jobs sequentially in a single thread since redis
commands can only be run by one thread at a time inside redis.

## Install

```sh
$ cargo build
$ # Mac:
$ redis-server --loadmodule ./target/debug/libredis_cron.dylib
$ # Linux:
$ redis-server --loadmodule ./target/debug/libredis_cron.so
```

## Usage

Available commands:

```
CRON.SCHEDULE <cron-expression> <redis-command>
CRON.UNSCHEDULE <job-id>
CRON.LIST
```

**Example**

```
$ redis-cli
127.0.0.1:6379> CRON.SCHEDULE "1/10 * * * * *" EVAL "return redis.call('set','foo','bar')" 0
"f04fefb1-ebf1-4d47-a582-f04963df994b"
127.0.0.1:6379> CRON.SCHEDULE "1/5 * * * * *" HINCRBY myhash field 1
"be428e43-5501-4eed-83c3-2c9ef3c52f6f"
127.0.0.1:6379> CRON.LIST
1) 1) f04fefb1-ebf1-4d47-a582-f04963df994b
   2) 1/10 * * * * *
   3) EVAL return redis.call('set','foo','bar') 0
2) 1) be428e43-5501-4eed-83c3-2c9ef3c52f6f
   2) 1/5 * * * * *
   3) HINCRBY myhash field 1
127.0.0.1:6379> CRON.UNSCHEDULE be428e43-5501-4eed-83c3-2c9ef3c52f6f
(integer) 1
127.0.0.1:6379> CRON.LIST
1) 1) f04fefb1-ebf1-4d47-a582-f04963df994b
   2) 1/10 * * * * *
   3) EVAL return redis.call('set','foo','bar') 0
127.0.0.1:6379> CRON.UNSCHEDULE f04fefb1-ebf1-4d47-a582-f04963df994b
(integer) 1
127.0.0.1:6379> CRON.LIST
(empty array)
```

Logs:

```log
66004:M 06 Mar 2021 15:33:22.814 # Server initialized
66004:M 06 Mar 2021 15:33:22.815 * Module 'cron' loaded from ./target/debug/libredis_cron.dylib
66004:M 06 Mar 2021 15:33:22.815 * Ready to accept connections
66004:M 06 Mar 2021 15:34:01.462 * <module> redis_cron: run: job_id=f04fefb1-ebf1-4d47-a582-f04963df994b; schedule=1/10 * * * * *; cmd=EVAL;
66004:M 06 Mar 2021 15:34:06.486 * <module> redis_cron: run: job_id=be428e43-5501-4eed-83c3-2c9ef3c52f6f; schedule=1/5 * * * * *; cmd=HINCRBY;
66004:M 06 Mar 2021 15:34:11.000 * <module> redis_cron: run: job_id=f04fefb1-ebf1-4d47-a582-f04963df994b; schedule=1/10 * * * * *; cmd=EVAL;
66004:M 06 Mar 2021 15:34:11.000 * <module> redis_cron: run: job_id=be428e43-5501-4eed-83c3-2c9ef3c52f6f; schedule=1/5 * * * * *; cmd=HINCRBY;
66004:M 06 Mar 2021 15:34:16.017 * <module> redis_cron: run: job_id=be428e43-5501-4eed-83c3-2c9ef3c52f6f; schedule=1/5 * * * * *; cmd=HINCRBY;
66004:M 06 Mar 2021 15:34:21.043 * <module> redis_cron: run: job_id=f04fefb1-ebf1-4d47-a582-f04963df994b; schedule=1/10 * * * * *; cmd=EVAL;
66004:M 06 Mar 2021 15:34:21.043 * <module> redis_cron: run: job_id=be428e43-5501-4eed-83c3-2c9ef3c52f6f; schedule=1/5 * * * * *; cmd=HINCRBY;
66004:M 06 Mar 2021 15:34:26.069 * <module> redis_cron: run: job_id=be428e43-5501-4eed-83c3-2c9ef3c52f6f; schedule=1/5 * * * * *; cmd=HINCRBY;
66004:M 06 Mar 2021 15:34:31.086 * <module> redis_cron: run: job_id=f04fefb1-ebf1-4d47-a582-f04963df994b; schedule=1/10 * * * * *; cmd=EVAL;
66004:M 06 Mar 2021 15:34:41.116 * <module> redis_cron: run: job_id=f04fefb1-ebf1-4d47-a582-f04963df994b; schedule=1/10 * * * * *; cmd=EVAL;
```

## Cron expression syntax

Creating a schedule for a job is done using the `FromStr` impl for the
`Schedule` type of the [cron](https://github.com/zslayton/cron) library.

The scheduling format is as follows:

```text
sec   min   hour   day of month   month   day of week   year
*     *     *      *              *       *             *
```

Time is specified for `UTC` and not your local timezone. Note that the year may
be omitted.

Comma separated values such as `5,8,10` represent more than one time value. So
for example, a schedule of `0 2,14,26 * * * *` would execute on the 2nd, 14th,
and 26th minute of every hour.

Ranges can be specified with a dash. A schedule of `0 0 * 5-10 * *` would
execute once per hour but only on day 5 through 10 of the month.

Day of the week can be specified as an abbreviation or the full name. A
schedule of `0 0 6 * * Sun,Sat` would execute at 6am on Sunday and Saturday.

### Reference

This project uses makes use of [redismodule-rs](https://github.com/RedisLabsModules/redismodule-rs)
