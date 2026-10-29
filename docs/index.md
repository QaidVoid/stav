---
layout: page
title: Stav
titleTemplate: Typed configuration that always finishes
sidebar: false
---

<Home>
<template #source>

<!-- file: app.stav -->

```stav
type Level = debug | info | warn | error

struct Upstream {
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
}

struct Root {
  listen:    Str = "127.0.0.1:8080"
  log_level: Level = info
  upstreams: Map<Str, Upstream> = {} block upstream
}

log_level = warn

upstream primary { url = "https://a.example.org", weight = 90 }
upstream backup { url = "https://b.example.org" }
```

</template>
<template #toml>

<!-- run: eval -o toml app.stav -->

```toml
listen = "127.0.0.1:8080"
log_level = "warn"

[upstreams.backup]
url = "https://b.example.org"
weight = 10

[upstreams.primary]
url = "https://a.example.org"
weight = 90
```

</template>
<template #json>

<!-- run: eval -o json app.stav -->

```json
{
  "listen": "127.0.0.1:8080",
  "log_level": "warn",
  "upstreams": {
    "backup": {
      "url": "https://b.example.org",
      "weight": 10
    },
    "primary": {
      "url": "https://a.example.org",
      "weight": 90
    }
  }
}
```

</template>
<template #yaml>

<!-- run: eval -o yaml app.stav -->

```yaml
listen: "127.0.0.1:8080"
log_level: warn
upstreams:
  backup:
    url: "https://b.example.org"
    weight: 10
  primary:
    url: "https://a.example.org"
    weight: 90
```

</template>
</Home>
