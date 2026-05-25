# xgraft

A CLI tool for importing source and header files into an MPLAB X IDE project.

It copies files into the project directory and automatically registers them in `configurations.xml`.

## Install

```bash
cargo install --path .
```

## Usage

```text
xgraft [OPTIONS] <PROJECT_PATH> [FILES]...
```

### Arguments

| Argument       | Description                                                                   |
| -------------- | ----------------------------------------------------------------------------- |
| `PROJECT_PATH` | A `.X` directory, or a parent directory containing exactly one `.X` directory |
| `FILES`        | Files to import (`.c`, `.h`, `.cpp`, `.hpp`, `.xlib`, or directories)         |

### Options

| Option          | Description                                                                  |
| --------------- | ---------------------------------------------------------------------------- |
| `-l`, `--library <FILE>` | Library files to import (`.xlib`, directories containing `.xlib`, or source/header files) |
| `-f`, `--force` | Force overwrite without confirmation                                         |

### Examples

```bash
# Import individual files
xgraft ./MyProject.X can.h can.c

# Specify the parent directory (must contain exactly one .X directory)
xgraft ./MyProject can.h can.c spi.hpp spi.cpp

# Import a package file using the library option (useful for aliases)
xgraft -l drivers.xlib ./MyProject.X

# Import a package file directly as a positional argument
xgraft ./MyProject.X drivers.xlib

# Import from a directory containing a single .xlib
xgraft ./MyProject.X ./libraries/can/

# Mix direct files and packages
xgraft ./MyProject.X uart.c drivers.xlib

# Force overwrite
xgraft --force ./MyProject.X can.h drivers.xlib
```

## How it works

1. Resolves the `.X` project directory from the specified path
2. Copies target files into the `.X` directory (existing files require confirmation unless `--force` is used)

   Copy destination rules (current implementation):

   - Files referenced in an `.xlib` are resolved relative to the `.xlib` file and validated, but when copied into the project they are placed at the root of the `.X` directory using the source file's basename only. Subdirectory structure from the source is not recreated.

     For example, given this `.xlib` content:

     ```text
     libraries/
     ├── delay/
     │   ├── delay.c
     │   └── delay.h
     └── drivers.xlib
     ```

     the files are copied as:

     ```text
     MyProject.X/
     ├── delay.c
     └── delay.h
     ```

   - When passing an individual file (not via an `.xlib`), the file is also copied into the root of the `.X` directory (e.g. `can/can.c` -> `MyProject.X/can.c`).

   - Note: because only the basename is used, files with identical basenames from different source directories will collide. The tool will prompt for overwrite (unless `--force`), but users should avoid duplicate basenames when expecting directory structure preservation.
3. Parses `nbproject/configurations.xml` and adds `<itemPath>` entries to the appropriate `logicalFolder`

  * `.h` / `.hpp` → `HeaderFiles` (XML `logicalFolder` target)
  * `.c` / `.cpp` → `SourceFiles` (XML `logicalFolder` target)
4. Skips files that are already registered in the XML

## `.xlib` Package Files

An `.xlib` file is a YAML-based package descriptor that lets you batch-import files with an optional logical folder hierarchy.

### Format

```yaml
# Root-level files are imported directly into SourceFiles / HeaderFiles
files:
  - delay/delay.c
  - delay/delay.h

# Named groups become nested logicalFolder nodes in MPLAB
Drivers:
  CAN:
    files:
      - can/can.c
      - can/can.h

  SPI:
    files:
      - spi/spi.c
      - spi/spi.h
```

### Rules

| Rule                                         | Description                                                                                  |
| -------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `files:` is a reserved key                   | It defines the list of source/header files at a given level                                   |
| Nested mappings are groups                   | Any YAML key that isn't `files:` is treated as a named group (logical folder)                 |
| Groups nest arbitrarily                      | `Drivers: CAN: files: [...]` creates `Drivers/CAN` in the MPLAB project tree                 |
| Root-level `files:`                          | Imported directly into the root `SourceFiles` / `HeaderFiles` folders                         |
| File classification                          | `.c`/`.cpp` → `SourceFiles`, `.h`/`.hpp` → `HeaderFiles`                                     |

### Path Resolution

All paths inside a `.xlib` file are resolved **relative to the `.xlib` file itself**.

```
libraries/
├── can/
│   ├── can.c
│   └── can.h
└── drivers.xlib    ← paths in here are relative to libraries/
```

### Logical Folder Mapping

Groups in `.xlib` map to nested `logicalFolder` nodes in MPLAB's `configurations.xml`.

Important implementation detail: the tool registers file entries in `configurations.xml` using the file's basename only (for example `can.c`), not the original relative path. The group names from the `.xlib` become nested `logicalFolder` nodes, but each `<itemPath>` contains the basename.

Example YAML:

```yaml
Drivers:
  CAN:
    files:
      - can/can.c
```

Results in a logical tree like:

```
Source Files
└── Drivers
    └── CAN
        └── can.c    (registered as `<itemPath>can.c</itemPath>`)
```

### Directory Discovery

When a directory is passed as an argument:

- xgraft searches the directory for `.xlib` files (non-recursive, top-level only)
- If exactly **one** `.xlib` is found, it is used
- If **none** are found, an error is returned
- If **multiple** are found, an error is returned listing the candidates

### Overwrite Behavior

- If a destination file already exists, xgraft prompts for confirmation
- Use `--force` (`-f`) to skip all prompts and overwrite automatically
- Files already registered in `configurations.xml` are skipped (idempotent)

### Importable file types

- Only source and header files with the extensions `.c`, `.cpp`, `.h`, and `.hpp` are allowed inside `files:` lists and anywhere files are referenced for import.
- Any other extension causes the command to fail with an error (fail-fast) rather than being silently ignored.
- Users should ensure only supported source/header file types are listed.

- The `.xlib` package descriptor itself is a YAML manifest and may contain nested groups and `files:` lists, but the files listed must be importable types as described above.

Example `.xlib` file

```yaml
# Root-level files
files:
  - delay/delay.c
  - delay/delay.h

# Grouped files -> logicalFolder: Drivers/CAN and Drivers/SPI
Drivers:
  CAN:
    files:
      - can/can.c
      - can/can.h

  SPI:
    files:
      - spi/spi.c
      - spi/spi.h
```

Command usage examples

- Import individual files into a project (copies to `.X` root and registers):

  ```bash
  xgraft ./MyProject.X can.h can.c
  ```

- Import a package `.xlib`:

  ```bash
  xgraft ./MyProject.X drivers.xlib
  ```

- Import by passing a directory that contains exactly one `.xlib` (non-recursive search):

  ```bash
  xgraft ./MyProject.X ./libraries/can/
  ```

- Force overwrite without prompts:

  ```bash
  xgraft --force ./MyProject.X can.h drivers.xlib
  ```

Notes

- When passing a directory, the tool searches the directory's top level for `.xlib` files and requires exactly one match.
- The tool validates that all files referenced in an `.xlib` exist relative to the `.xlib` location and will fail if any are missing or have unsupported extensions.
