# MC-log-tools
Rust based minecraft log tools 
for searching chat messages c:

# Building (Win)
1. Install Rust
2. Install Visual Studio C++ Build tools
3. Building as release
```bash
cargo build --release
```

# LogTools
## Create DataBase

```bash
logtools3 cb <PATHS>... <DB>
```

### Options
-f, --from-file <FILE> (file with log paths)

------------

### Examples
Creating base with one log dir, 2 or more dirs or logs paths file
```bash
logtools3 cb "%AppData%\.minecraft\logs" my_base
```
```bash
logtools3 cb "C:\Minecraft\LunarClient\logs" "C:\Prism\logs" my_base2
```
```bash
logtools3 cb -f "C:\log_paths.txt" my_base3
```

----------

Its all for now :#
