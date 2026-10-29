# Application configuration

This example configures a web application: an HTTP server, a database, background workers and logging. The schema lives in a library, the configuration is split into a base file and a production part, and one command renders it to TOML, JSON or YAML for whatever reads it.

## The schema

The library declares the shape of the configuration. Refinements state what values are valid, and defaults make most fields optional.

<!-- file: lib/schema.stav -->

```stav
## How much the application logs.
type Level = debug | info | warn | error

## Where log lines go.
type Sink =
  | stderr
  | file(path: Str where nonEmpty, rotate_mb: Int where 1..=1024 = 64)

struct Http {
  host:            Str = "127.0.0.1"
  port:            Int where 1..=65535 = 8080
  request_timeout: Int where 1..=300 = 30
  allowed_origins: Set<Str> = []
}

struct Database {
  url:       Str where matches(r"postgres://.+")
  pool_size: Int where 1..=200 = 10
  replicas:  List<Str> = []
}

struct Worker {
  queue:       Str where nonEmpty
  concurrency: Int where 1..=64 = 4
  schedule:    Option<Str>
}

struct Logging {
  level: Level = info
  sink:  Sink = stderr
}

## The configuration of one application.
struct App {
  name:     Str where matches(r"[a-z][a-z0-9-]*")
  http:     Http = {}
  database: Database
  workers:  Map<Str, Worker> = {} block worker
  logging:  Logging = {}
  features: Map<Str, Bool> = {}
}

## A worker for each queue, sharing one concurrency.
fn workers(queues: List<Str>, concurrency: Int where 1..=64) -> Part<App> = part {
  for queue in queues {
    worker "#{queue}" {
      queue       = queue
      concurrency = default(concurrency)
    }
  }
}
```

## The base configuration

The entry file imports the schema. Its root struct holds the application's configuration in one field, `app`, and the `app { }` block fills it in. Everything set here is a plain contribution.

<!-- file: app.stav -->

```stav
import "./lib/schema.stav" as schema

struct Root {
  app: schema.App
}

let name = "inventory"

app {
  name = name

  http {
    port            = 8080
    allowed_origins = ["https://#{name}.example.org"]
  }

  database {
    url = "postgres://localhost/#{name}"
  }

  include schema.workers(["email", "thumbnails"], concurrency = 2)

  worker reports {
    queue    = "reports"
    schedule = "0 3 * * *"
  }

  features { search = true, exports = false }

  include "./production.stav"
}
```

## The production part

A part file holds what differs in production. It is included inside `app { }`, so its statements are typed against `schema.App`. `force(...)` replaces the database URL, and the plain `concurrency = 8` wins over the part's `default(2)` without a conflict.

<!-- file: production.stav -->

```stav
http {
  host            = "0.0.0.0"
  allowed_origins = ["https://admin.example.org"]
}

database {
  url       = force("postgres://db.internal/inventory")
  pool_size = 50
  replicas  = ["postgres://replica-1.internal/inventory"]
}

worker thumbnails {
  concurrency = 8
}

logging {
  level = warn
  sink  = file(path = "/var/log/inventory.log")
}

features.exports = force(true)
```

## The rendered value

`stav check` confirms the program is valid without evaluating it:

<!-- run: check app.stav -->

`stav eval` prints the value, and `--path app` selects the application's part of it. Choose the format the application reads:

```sh
stav eval --path app -o toml app.stav
```

::: code-group

<!-- run: eval --path app -o toml app.stav -->

```toml [TOML]
name = "inventory"

[http]
host = "0.0.0.0"
port = 8080
request_timeout = 30
allowed_origins = ["https://admin.example.org", "https://inventory.example.org"]

[database]
url = "postgres://db.internal/inventory"
pool_size = 50
replicas = ["postgres://replica-1.internal/inventory"]

[workers.email]
queue = "email"
concurrency = 2

[workers.reports]
queue = "reports"
concurrency = 4
schedule = "0 3 * * *"

[workers.thumbnails]
queue = "thumbnails"
concurrency = 8

[logging]
level = "warn"

[logging.sink.file]
path = "/var/log/inventory.log"
rotate_mb = 64

[features]
exports = true
search = true
```

<!-- run: eval --path app app.stav -->

```json [JSON]
{
  "name": "inventory",
  "http": {
    "host": "0.0.0.0",
    "port": 8080,
    "request_timeout": 30,
    "allowed_origins": [
      "https://admin.example.org",
      "https://inventory.example.org"
    ]
  },
  "database": {
    "url": "postgres://db.internal/inventory",
    "pool_size": 50,
    "replicas": [
      "postgres://replica-1.internal/inventory"
    ]
  },
  "workers": {
    "email": {
      "queue": "email",
      "concurrency": 2,
      "schedule": null
    },
    "reports": {
      "queue": "reports",
      "concurrency": 4,
      "schedule": "0 3 * * *"
    },
    "thumbnails": {
      "queue": "thumbnails",
      "concurrency": 8,
      "schedule": null
    }
  },
  "logging": {
    "level": "warn",
    "sink": {
      "file": {
        "path": "/var/log/inventory.log",
        "rotate_mb": 64
      }
    }
  },
  "features": {
    "exports": true,
    "search": true
  }
}
```

<!-- run: eval --path app -o yaml app.stav -->

```yaml [YAML]
name: inventory
http:
  host: "0.0.0.0"
  port: 8080
  request_timeout: 30
  allowed_origins:
    - "https://admin.example.org"
    - "https://inventory.example.org"
database:
  url: "postgres://db.internal/inventory"
  pool_size: 50
  replicas:
    - "postgres://replica-1.internal/inventory"
workers:
  email:
    queue: email
    concurrency: 2
    schedule: null
  reports:
    queue: reports
    concurrency: 4
    schedule: "0 3 * * *"
  thumbnails:
    queue: thumbnails
    concurrency: 8
    schedule: null
logging:
  level: warn
  sink:
    file:
      path: /var/log/inventory.log
      rotate_mb: 64
features:
  exports: true
  search: true
```

:::

A few things to notice in the output:

- `allowed_origins` is a set, so the two contributions from the base file and the production part are combined and sorted.
- The workers created by the part keep their default concurrency of 2, except `thumbnails`, where the production part's plain assignment wins.
- `schedule` is an `Option<Str>`. Only `reports` sets it. TOML leaves absent options out, while JSON and YAML print `null`.
- `sink` is a sum type with a payload. It renders as a table keyed by the variant name, with the default `rotate_mb` filled in.

## When something is wrong

Suppose the production part set `pool_size = 500` instead. The refinement on `pool_size` catches it, and the diagnostic points at both the value and the constraint:

<!-- file: production.stav -->

```stav
database {
  url       = force("postgres://db.internal/inventory")
  pool_size = 500
}
```

<!-- run: eval app.stav -->

```text
error[E0803]: value violates a refinement
   ┌─ production.stav:3:15
   │
 3 │   pool_size = 500
   │               ^^^
   │               │
   │               500 is not in 1..=200
   │
   ┌─ lib/schema.stav:18:14
   │
18 │   pool_size: Int where 1..=200 = 10
   │              ----------------- constraint declared here
```

Nothing is printed on standard output, and the command exits with status 1. A deployment script can rely on that: either the whole configuration is valid, or nothing is written.
