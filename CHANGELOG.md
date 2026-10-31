
## [0.2.0](https://github.com/QaidVoid/stav/compare/v0.1.0...v0.2.0) - 2026-10-29

### ⛰️  Features

- *(check)* Resolve symbols and references past unfit bindings - ([f581799](https://github.com/QaidVoid/stav/commit/f5817994ef83647e50f0a0cfe28a938adf157cf5))
- *(eval)* Point spans inside variant payloads - ([bdfe2ca](https://github.com/QaidVoid/stav/commit/bdfe2caf39f8f6fce96c5dc7701228e243e4b0aa))
- *(eval)* Fill declared defaults in host struct values - ([28ff2c0](https://github.com/QaidVoid/stav/commit/28ff2c04e390694bb2ddd6946f8ee94c7207e7d8))
- *(eval)* Supply host module values at evaluation time - ([6c157cc](https://github.com/QaidVoid/stav/commit/6c157cce009be288d7cca139ccd5572f480f60b3))
- *(eval)* Report the winning priority of an attribute - ([59d0229](https://github.com/QaidVoid/stav/commit/59d0229a336ef8ad6d257febb699253a1de11084))
- *(eval)* Return the path of every symbol occurrence - ([ada2561](https://github.com/QaidVoid/stav/commit/ada25611eb1bc5e3f6d5547b267dabeb7de830e4))
- *(eval)* Point spans at list elements and literal entries - ([19ce480](https://github.com/QaidVoid/stav/commit/19ce480eb41a02268b796075a034f5503611ed48))
- *(eval)* [**breaking**] Merge each part once into a block - ([b34ea21](https://github.com/QaidVoid/stav/commit/b34ea21f5777b0a6ac9be655310abf1ea5192f38))
- Let hosts build and replace diagnostics - ([e95e64b](https://github.com/QaidVoid/stav/commit/e95e64b82ab5b0e19ba50005519cd087accc630a))

### 🐛 Bug Fixes

- *(check)* Report a let used too early instead of a symbol - ([ae015f9](https://github.com/QaidVoid/stav/commit/ae015f9bd5acbe833836b3bb68f35ac087a6eb17))

### 🚜 Refactor

- *(eval)* Share the evaluation of list literal elements - ([3ef01ef](https://github.com/QaidVoid/stav/commit/3ef01ef9a275e59350e54b42702afbd8cf2dff5f))

### 📚 Documentation

- *(eval)* State how host values, priorities and keys behave - ([5a14905](https://github.com/QaidVoid/stav/commit/5a149054f10cbd40a606803a75575a0ec9b68259))

### 🧪 Testing

- Replace real program names in new fixtures and docs - ([685e91c](https://github.com/QaidVoid/stav/commit/685e91cb61f32011b2aa9d685a3ca8cae335e6b0))

## [0.1.0] - 2026-10-29

### ⛰️  Features

- *(ast)* Add a typed view of the syntax tree - ([a146b69](https://github.com/QaidVoid/stav/commit/a146b69ae2702cbef0abdb94cdbc4f6daceac062))
- *(check)* List source inputs with BLAKE3 hashes - ([0b100c0](https://github.com/QaidVoid/stav/commit/0b100c0a0ee5a0ea2d783701f68c17d2b8e3f175))
- *(check)* Reject recursion and self dependency cycles - ([18cba76](https://github.com/QaidVoid/stav/commit/18cba76089997346cfd04d45221f4a9236fdec14))
- *(check)* Type check declarations, expressions and bodies - ([8ff59a6](https://github.com/QaidVoid/stav/commit/8ff59a6e950cdee70464bcaab0f2c152823f0d35))
- *(check)* Load roots, imports and part files through a loader - ([6dac29b](https://github.com/QaidVoid/stav/commit/6dac29b6c797e48a832c5a56db416fecd13b744a))
- *(cli)* Add the stav command with fmt, check and eval - ([16ccde1](https://github.com/QaidVoid/stav/commit/16ccde1385ece022bce44b22ec3e716003bcf79f))
- *(eval)* Render the attribute at a path - ([0d6fef7](https://github.com/QaidVoid/stav/commit/0d6fef7f3f053d58a62fd8074ba584efe9326d56))
- *(eval)* Evaluate checked programs into merged values - ([eb9a315](https://github.com/QaidVoid/stav/commit/eb9a315e3ffc8caa788c75a203bee578ad058b76))
- *(format)* Format source text into its canonical layout - ([9bc2190](https://github.com/QaidVoid/stav/commit/9bc21904080ae679da0e7f89039d409b739bdf80))
- *(lexer)* Lex strings with interpolation and indentation - ([070cf75](https://github.com/QaidVoid/stav/commit/070cf75a3f4c4cb8d5efa3873bf6f40b995007a5))
- *(lexer)* Lex identifiers, symbol words, numbers and punctuation - ([a764cdd](https://github.com/QaidVoid/stav/commit/a764cdd45cdf616a4e2a0a518b003ac8c0bc3ed8))
- *(parser)* Parse source into a lossless syntax tree - ([887d042](https://github.com/QaidVoid/stav/commit/887d04289f78b0b8de6fdae2e09f33eaf555afd3))
- *(value)* Convert values to Rust types through serde - ([4fc66df](https://github.com/QaidVoid/stav/commit/4fc66dfa29a247caec9fbb2d5af7f4314f8785e2))
- *(value)* Add values with JSON, TOML and YAML rendering - ([c9c24e3](https://github.com/QaidVoid/stav/commit/c9c24e37106e8d76cd93914168b9f0c5af1b81d1))
- Expose the source size limit and apply it in fmt - ([03d7217](https://github.com/QaidVoid/stav/commit/03d721754fb1d550b23af04e26730d6ec6c4aadf))
- Parse attribute paths in the form they display in - ([a8ccbc1](https://github.com/QaidVoid/stav/commit/a8ccbc1078f0691d5ab63ce9160b9ad3b514bf1b))

### 🐛 Bug Fixes

- *(check)* Refuse entries outside their root or listed twice - ([23e837a](https://github.com/QaidVoid/stav/commit/23e837ad3d0b330943011a587500cf8023d51bd9))
- *(check)* Report repeated includes, arity and markers once each - ([b2ca0c3](https://github.com/QaidVoid/stav/commit/b2ca0c34eb34456f9e2a805c25323bfad01d446b))
- *(check)* Resolve let bindings, map keys and set comprehensions - ([b4f4627](https://github.com/QaidVoid/stav/commit/b4f46273b8ab2faf084121c8a1c596ae700e1751))
- *(check)* Keep invalid literal patterns from covering other arms - ([357a1ff](https://github.com/QaidVoid/stav/commit/357a1ffe5e8c258c8303dc12fb889855bd210294))
- *(check)* Find cycles through omitted defaults - ([74f6efd](https://github.com/QaidVoid/stav/commit/74f6efdb429570370b347e679cd5c1ae32fc0fdb))
- *(check)* Resolve refinements of types written in bodies - ([9993a11](https://github.com/QaidVoid/stav/commit/9993a11fa0a2b318783170fd1ff6b4f32d49c933))
- *(check)* Check long chains of files without recursing - ([f9c979d](https://github.com/QaidVoid/stav/commit/f9c979d1c650273aec7bb6e3296d85e1ecb3c4e4))
- *(cli)* Skip hidden entries when fmt walks a directory - ([bc7b524](https://github.com/QaidVoid/stav/commit/bc7b5246e87e31b03ca688521a70ab777bf9b7ef))
- *(cli)* Resolve --root-dir before locating the entry - ([96a5686](https://github.com/QaidVoid/stav/commit/96a5686fb7961fbf62616cdf8eac0b665e3cf5e4))
- *(eval)* Name the whole path of a broken reference - ([d0ed3fb](https://github.com/QaidVoid/stav/commit/d0ed3fbb342b4d2d58950a229b66637a197aab58))
- *(eval)* Keep map keys and local lookups linear and counted - ([309b1e1](https://github.com/QaidVoid/stav/commit/309b1e1c0567702c39998ae66efe805ae11405dd))
- *(eval)* Enforce refinements of struct fields and results - ([9cb6be2](https://github.com/QaidVoid/stav/commit/9cb6be294cdec62afc2ee52484f75b4b9ddb122d))
- *(format)* Keep blank lines between let bindings - ([6c832c8](https://github.com/QaidVoid/stav/commit/6c832c8a3a7f937982a0616b1e5d1f9d0130005b))
- *(format)* Keep comments before braces and bodies in place - ([5b68f0a](https://github.com/QaidVoid/stav/commit/5b68f0adabee04e5fd6ca89145b6b4d2b9a6fbed))
- *(lexer)* Explain a multi-line string in a single-line one - ([dda12ec](https://github.com/QaidVoid/stav/commit/dda12ec37b137b793190ba1e69dc224c8ad56fab))
- *(lexer)* End nested multi-line strings at single-line line feeds - ([25fb3fe](https://github.com/QaidVoid/stav/commit/25fb3fe4401809012804cd7241bdd4bea1212567))
- *(lexer)* Sharpen lexical diagnostics and token docs - ([17b3b47](https://github.com/QaidVoid/stav/commit/17b3b47cb42b1e05c5f7d6935dd5d639e1d25113))
- *(lexer)* End path literals at commas and equals signs - ([2cdc194](https://github.com/QaidVoid/stav/commit/2cdc19448d7a37b4f74920c8adb6c36d8e8dfe84))
- *(lexer)* Allow digit separators only between two digits - ([475932a](https://github.com/QaidVoid/stav/commit/475932ab88d0589ead167370c5dfbea68013b9e6))
- *(lexer)* End single-line strings at the line end through nesting - ([9fa40a3](https://github.com/QaidVoid/stav/commit/9fa40a3f5d1dfb4031e27f765c37127086c566a5))
- *(lexer)* Strip multi-line indentation in linear time - ([ad73cda](https://github.com/QaidVoid/stav/commit/ad73cda5b85b538bb4f8aad6419b47216155db14))
- *(parser)* Report a symbol word import alias with E0206 - ([0b08461](https://github.com/QaidVoid/stav/commit/0b08461732d5d144d9458f64d2e95a6e57c07200))
- *(parser)* Report misplaced arguments once per list - ([6679d56](https://github.com/QaidVoid/stav/commit/6679d568c71a640fa801952621c7e5dce6e6e5b7))
- *(parser)* Report stray commas between elements - ([80e98aa](https://github.com/QaidVoid/stav/commit/80e98aac7ad616fea23f9b6646957c1fd5943fc5))
- Give each item placement rule its own error code - ([fd80093](https://github.com/QaidVoid/stav/commit/fd80093aa083b92fe7c0ac26526a4a93abbce67c))

### 🚜 Refactor

- *(check)* Drop dead fact fields and document declarations - ([1fb1e5d](https://github.com/QaidVoid/stav/commit/1fb1e5d53af8e9d94910de4900dc6ffa3bfb41f8))
- *(check)* Return input sources instead of hashes - ([4369438](https://github.com/QaidVoid/stav/commit/4369438658215b952c848471cc6ae80099bcbae4))
- *(format)* Drop an unreachable arm and fix a doc comment - ([f8fa017](https://github.com/QaidVoid/stav/commit/f8fa017087361fbd6326988cf5ef2fef48f451c2))
- *(syntax)* Keep the root index in the tree - ([99f48f4](https://github.com/QaidVoid/stav/commit/99f48f43dfca271e28369255044e6f521d6c405d))

### 📚 Documentation

- *(cli)* Show the default fuel and a neutral path example - ([920689a](https://github.com/QaidVoid/stav/commit/920689a8b5ade609d6f1314559886871e92593c1))
- Rewrite the README and crate docs for a first release - ([3dae2ef](https://github.com/QaidVoid/stav/commit/3dae2efd7554fd250b43b045e49e5997acaabd72))
- Use invented names in doc examples - ([caac08b](https://github.com/QaidVoid/stav/commit/caac08b71db050d1257fdc8e8daa350cd62cbbc4))

### ⚡ Performance

- *(check)* Reuse one parent array when finding cycles - ([066ba55](https://github.com/QaidVoid/stav/commit/066ba558fd87c851d362d9b8a5f2de5b0013828a))
- *(check)* Index the exhaustiveness matrix by its first column - ([66d6bbf](https://github.com/QaidVoid/stav/commit/66d6bbf67046bb1217a693f5b1686ca99311a1b6))

### 🧪 Testing

- *(check)* Use neutral names in suggestion and property tests - ([8d03d8a](https://github.com/QaidVoid/stav/commit/8d03d8a68dd9a0b77973d21cee6b8e883dc6cc06))
- *(check)* Add property tests and a checker fuzz target - ([f60457b](https://github.com/QaidVoid/stav/commit/f60457b1361b76ce429d1c040ac4b76b43bc7499))
- *(cli)* Cover warnings and use neutral fixture names - ([e1c5558](https://github.com/QaidVoid/stav/commit/e1c5558520db3753f705e0b8d15dbae85c499662))
- *(eval)* Pin refinements of defaulted payload fields - ([0257f19](https://github.com/QaidVoid/stav/commit/0257f192e5d9bb8e325571162fc4ac500f62baff))
- *(eval)* Add a benchmark of 2 000 generated recipes - ([c4bfc80](https://github.com/QaidVoid/stav/commit/c4bfc801820665dc399cf3f95ef5874babe17345))
- *(eval)* Cover evaluation with fixtures and property tests - ([ee0f6fc](https://github.com/QaidVoid/stav/commit/ee0f6fc5c230baa59b6c32a48de0354f1a5c98d5))
- *(format)* Share one meaning helper between formatter tests - ([c65e069](https://github.com/QaidVoid/stav/commit/c65e0690fd6463b1ec67e19bad7f580e6ff28c83))
- *(format)* Cover the printer with small documents - ([c525d70](https://github.com/QaidVoid/stav/commit/c525d708834c5932ddf2960532b90992915008b5))
- *(lexer)* Check values of nested multi-line strings - ([5240e98](https://github.com/QaidVoid/stav/commit/5240e98df8b9fa2926ed506f7ecad1bea23dd323))
- *(lexer)* Cover indentation of nested multi-line strings - ([e5c3825](https://github.com/QaidVoid/stav/commit/e5c3825aea9404b6a8a02cf14890810e7258745e))
- *(lexer)* Print token kinds last in snapshots - ([80081ae](https://github.com/QaidVoid/stav/commit/80081ae03a01b602744094064de00429d460ea3a))
- *(parser)* Check exact node spans and child order - ([ce20473](https://github.com/QaidVoid/stav/commit/ce20473689de799767be56ce2913edc4cca71485))
- Replace real program names in fixtures and tests - ([521ac32](https://github.com/QaidVoid/stav/commit/521ac327d080af5fc204318a2fd3653c038e14a5))

### ⚙️ Miscellaneous Tasks

- Add crates.io metadata and keep the binary out of docs - ([cddc4cf](https://github.com/QaidVoid/stav/commit/cddc4cfe9ab3b584cc1149dc45c341a4e5b87662))
- Bootstrap cargo workspace - ([eac7a9d](https://github.com/QaidVoid/stav/commit/eac7a9d1cf0c368415862b53493e55fa455084ea))

### Build

- Declare dependencies once in the workspace - ([9b52ecc](https://github.com/QaidVoid/stav/commit/9b52ecc56e418932479fa47828ce88c8b168b456))
