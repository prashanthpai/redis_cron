# redis_cron

redis_cron is a simple cron expression based job scheduler for Redis that runs
inside redis as a module. It uses similar syntax as a regular cron and allows
you to schedule redis commands directly on redis. This project is inspired from
PostgreSQL extension [pg_cron](https://github.com/citusdata/pg_cron).

redis_cron runs scheduled jobs sequentially in a single thread since redis
commands can only be run by one thread at a time inside redis.

NOTE: This project is experimental and not complete yet.

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
```

**Example**

Run some redis command every 10 seconds:

```
$ redis-cli
127.0.0.1:6379> CRON.SCHEDULE "1/10 * * * * *" HINCRBY myhash field 1
"c5e68b53-e969-4280-bcef-21703def0994"
127.0.0.1:6379> HGETALL myhash
(empty array)
127.0.0.1:6379> HGETALL myhash
1) "field"
2) "1"
127.0.0.1:6379> HGETALL myhash
1) "field"
2) "2"
127.0.0.1:6379> CRON.UNSCHEDULE c5e68b53-e969-4280-bcef-21703def0994
(nil)
```

Scheduled job information is stored for future reference by users:

```
127.0.0.1:6379> HGETALL redis_cron::jobid_expr
1) "2605b43a-ee81-4a34-a147-8e4cdc016709"
2) "1/10 * * * * *"
127.0.0.1:6379> HGETALL redis_cron::jobid_cmd
1) "2605b43a-ee81-4a34-a147-8e4cdc016709"
2) "HINCRBY myhash field 1"
127.0.0.1:6379>
```

The above keys are only for reference and not (yet) used to load and run jobs.

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

This project uses:

* [job_scheduler](github.com/lholden/job_scheduler)
* [redismodule-rs](https://github.com/RedisLabsModules/redismodule-rs)
