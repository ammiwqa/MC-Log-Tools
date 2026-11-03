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
<br><br>

# LogTools Functional:
\> Minecraft log parser!

  ```bash
  logtools <COMMAND> <OPTIONS>
  ```  
     


## 1. Create DataBase  
\>   Creating  base from minecraft logs.
```bash
logtools cb <BASE> <PATHS>...
```
> \<BASE> - your LT base name

---

### Options

#### File with log paths
Using text file with paths to directories with minecraft logs
```
-f, --from-file <path-to-file>
```

 ```log_paths.txt```
>  ```txt
> C:\Minecraft\LunarClient\logs
> C:\Prism\logs
> ...
>  ```   

---

### Examples
Creating base with one log dir, 2 or more dirs or logs paths file
```bash
logtools cb my_base "%AppData%\.minecraft\logs"
```
```bash
logtools cb my_base2 "C:\Minecraft\LunarClient\logs" "C:\Prism\logs"
```
```bash
logtools cb my_base3 -f "C:\log_paths.txt"
```

---

<br><br>

## 2. Search

\>    Searching text in existing LT base.  
\>    Output: .txt results file.

```bash
logtools search <BASE> <TEXT>
```

Logic is supported:
+ `&` (AND)  
+ `|` (OR)  
+ `!` (NOT)  
+ `()` (Ordering)  
+ `*` (N skipped chars)  


---

### Examples
Searching messages what includes "hello" and "cat" from my_base
```bash
logtools search my_base "hello & cat"
```
---



<br><br>

## 3. Update base
\>    Updating base with new logs if they appeared in the database directories.  

```bash
logtools update <BASE>
```

---


<br><br>
## 4. Discover
\>    Discovering minecraft directions with logs on your disk from ROOT.  
\>    Using [Everething](#everething-1) if it configured.  
\>    Output: .txt file with paths to found bases (using in [Create Base](#1-create-database) with ```--from-file``` flag)

```bash
logtools discover <ROOT>
```
---

### Options

#### Output file
Path to output file. Default: ```%AppData%\logtools\results\mc_log_dirs.txt```
```
-o, --out <file>
```
  
#### Everething  
Path to ```Everything64.dll``` or ```es.exe```
```
--es <path-to-everething>
```

  
#### No Everething  
Disable Everething if it configured or found automaticly.
```
--no-everything
```


---

### Examples
<br>Discovering all disk (with [everething](#everething-1) if it [configured](#avaliable-config-options)):  
```bash
logtools discover
```

Discovering ```%AppData%``` windows directory:  
```bash
logtools discover %AppData%
```

Discovering path ```D:/Minecraft``` and output in ```D:/Minecraft/paths.txt```:  
```bash
logtools discover --out "D:/paths.txt" "D:/Minecraft"
```

Discovering all computer with [Everething](#everething-1):  
```bash
logtools discover --es "C:/Everything-SDK/dll/Everything64.dll"
```

---

<br><br>
## 5. Base list
\>    Show list with existing LT bases.

```bash
logtools list
```

Output example:  
```text
NAME                             LINES    PATHS     CREATED                EDITED
───────────────────────────────────────────────────────────────────────────────────────────────
my_base                          28133828   354     2026-06-26 01:30:29    2026-10-03 20:11:35
test                               543384    23     2026-10-29 12:10:20    2026-11-03 22:10:20
───────────────────────────────────────────────────────────────────────────────────────────────
Total: 2 base(s).
```

---
<br><br>

## 6. Delete base
\>    Delete base. Needs confirmation by [default](#avaliable-config-options).

```bash
logtools delete <BASE> 
```

---
<br><br>  
## 7. Configuration
\>    Configuration for logools.

```bash
logtools config <OPTION> <VARIABLE>
```

#### Avaliable config options:
 + ```everything_dll``` - path to [Everething64.dll](#everething-1)
 + ```confirm_delete``` - deletion confirmation (```true```/```false```)   [```true``` by default]

<br> Display all avaliable options:
```bash
logtools config
```

---

<br><br>

## 8. Help
\>    Show help for logtools.  

 Show global help:
```bash
logtools help
```
 Show command help:
```bash
logtools <COMMAND> --help
```

---

<br><br>

## Everething
Logtools using Everething in [discovery](#4-discover) module.  
You can download Everething here:
 + [Everething SDK](https://www.voidtools.com/ru-ru/downloads/#sdk)  (Everething64.dll)  
 Extract zip -> Open `dll` directory -> `Everething64.dll` here.
 + [ES for command prompt](https://www.voidtools.com/ru-ru/downloads/#cli)  (es.exe)  
 Extract zip -> `es.exe` here.
 
---


<br>_Its all for now :#_
