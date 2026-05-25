# xgraft

MPLAB X IDE プロジェクトにソース／ヘッダファイルを取り込むための CLI ツールです。

指定したファイルを `.X` プロジェクトディレクトリへコピーし、`nbproject/configurations.xml` に `<itemPath>` エントリを自動で追加します。

## インストール

```bash
cargo install --path .
```

## 使い方

```text
xgraft [OPTIONS] <PROJECT_PATH> [FILES]...
```

### 引数

| 引数 | 説明 |
| ---- | ---- |
| `PROJECT_PATH` | `.X` ディレクトリ、または `.X` を1つだけ含む親ディレクトリ |
| `FILES` | インポートするファイル（`.c`, `.h`, `.cpp`, `.hpp`, `.xlib`、または `.xlib` を含むディレクトリ） |

### オプション

| オプション | 説明 |
| --- | --- |
| `-l`, `--library <FILE>` | インポートするライブラリファイル（`.xlib`、`.xlib` を含むディレクトリ、またはソース/ヘッダファイル） |
| `-f`, `--force` | 上書きを確認せずに強制実行 |

### 例

```bash
# 個別ファイルをインポート
xgraft ./MyProject.X can.h can.c

# 親ディレクトリを指定（ただしその中に .X は1つだけ）
xgraft ./MyProject can.h can.c spi.hpp spi.cpp

# ライブラリ指定オプションを使用してパッケージファイルをインポート（alias等に便利）
xgraft -l drivers.xlib ./MyProject.X

# パッケージファイルを位置引数として直接インポート
xgraft ./MyProject.X drivers.xlib

# .xlib を含むディレクトリを指定
xgraft ./MyProject.X ./libraries/can/

# 個別ファイルとパッケージの混在
xgraft ./MyProject.X uart.c drivers.xlib

# 強制上書き
xgraft --force ./MyProject.X can.h drivers.xlib
```

## 動作概要

1. 指定したパスから `.X` プロジェクトディレクトリを解決します。
2. 対象ファイルを `.X` ディレクトリへコピーします（既存ファイルは `--force` を指定しない場合、確認を求めます）。

   コピー先ルール（現行実装）:

   - `.xlib` 内で参照されるファイルは `.xlib` の所在ディレクトリを基準に解決・検証されますが、プロジェクトへコピーする際はファイルのベースネーム（basename）だけを使い、`.X` のルートに配置します。ソース側のサブディレクトリ構造は再現されません。

     例えば次のような構成の場合：

     ```text
     libraries/
     ├── delay/
     │   ├── delay.c
     │   └── delay.h
     └── drivers.xlib
     ```

     コピー後は次のようになります：

     ```text
     MyProject.X/
     ├── delay.c
     └── delay.h
     ```

   - 個別ファイルを直接指定した場合も同様にベースネームだけを使って `.X` ルートへコピーします（例：`can/can.c` → `MyProject.X/can.c`）。

   - 注意: ベースネームのみを使うため、異なるディレクトリにある同名ファイルは衝突します。`--force` がない場合は上書き確認が行われますが、ディレクトリ構造を期待する場合は同名衝突に注意してください。
3. `nbproject/configurations.xml` を解析し、適切な `logicalFolder` に `<itemPath>` 要素を追加します。

   * `.h` / `.hpp` → `HeaderFiles`
   * `.c` / `.cpp` → `SourceFiles`
4. すでに XML に登録されているファイルはスキップします（冪等性あり）。

## `.xlib` パッケージファイル

`.xlib` は YAML ベースのパッケージ記述子で、ファイルの一括インポートと論理フォルダ階層の指定を行えます。

### 形式

```yaml
# ルートレベルの files は SourceFiles / HeaderFiles に直接登録される
files:
  - delay/delay.c
  - delay/delay.h

# 名前付きグループは MPLAB の logicalFolder に対応するネストを作る
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

### ルール

| ルール | 説明 |
| --- | --- |
| `files:` は予約語 | そのレベルでインポートするソース／ヘッダ一覧を定義します |
| ネストされたマッピングはグループ | `files:` 以外の YAML キーは論理フォルダ名として扱います |
| グループは任意深さでネスト可能 | 例：`Drivers: CAN: files: [...]` → `Drivers/CAN` |
| ルートの `files:` | ルートの `SourceFiles` / `HeaderFiles` 配下へ登録されます |
| ファイル分類 | `.c`/`.cpp` → `SourceFiles`, `.h`/`.hpp` → `HeaderFiles` |

### パス解決

`.xlib` 内のすべてのパスは、その `.xlib` ファイル自身の所在ディレクトリを基準に解決されます。

```
libraries/
├── can/
│   ├── can.c
│   └── can.h
└── drivers.xlib    ← drivers.xlib 内のパスは libraries/ を基準に解釈される
```

### Logical Folder マッピング

`.xlib` のグループは MPLAB の `logicalFolder` ノードにマップされます。

実装上の重要な点: `configurations.xml` に登録される `<itemPath>` はファイルのベースネーム（例: `can.c`）のみです。グループ名は `logicalFolder` のネストとして表現されますが、`<itemPath>` に元の相対パスは書かれません。

例:

```yaml
Drivers:
  CAN:
    files:
      - can/can.c
```

結果として表示される論理ツリー例：

```
Source Files
└── Drivers
    └── CAN
        └── can.c    (登録は `<itemPath>can.c</itemPath>`)
```

### ディレクトリ探索

- 引数にディレクトリが指定された場合、トップレベル（非再帰）で `.xlib` を検索します。
- ちょうど **1 個** の `.xlib` が見つかった場合、それを使用します。
- 見つからない場合はエラーを返します。
- 複数見つかった場合は候補を列挙してエラーを返します。

### 上書き動作

- 既に同名ファイルが存在する場合、`--force` がないと確認を求めます。
- `--force` を指定すると全て自動で上書きします。
- `configurations.xml` に既に登録されているファイルは追加処理時にスキップされます（冪等性）。

### インポート可能なファイル種別

- `files:` リストやインポート対象として参照されるファイルは、拡張子が `.c`, `.cpp`, `.h`, `.hpp` のソース／ヘッダのみ許可されます。
- それ以外の拡張子が参照されているときは、コマンドはエラーで失敗します（fail-fast）。
- 利用者は `files:` には必ず対応したソース／ヘッダファイルのみを列挙してください。

---

`.xlib` の例

```yaml
# ルートレベルの files は SourceFiles / HeaderFiles に直接登録される
files:
  - delay/delay.c
  - delay/delay.h

# 名前付きグループは MPLAB の logicalFolder に対応するネストを作る
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

コマンド使用例

- 個別ファイルをプロジェクトにインポート（`.X` ルートにコピーして登録）:

  ```bash
  xgraft ./MyProject.X can.h can.c
  ```

- パッケージ `.xlib` をインポート:

  ```bash
  xgraft ./MyProject.X drivers.xlib
  ```

- `.xlib` を含むディレクトリを指定（トップレベルのみ検索、ちょうど1つの `.xlib` が必要）:

  ```bash
  xgraft ./MyProject.X ./libraries/can/
  ```

- 確認なしで上書き（強制）:

  ```bash
  xgraft --force ./MyProject.X can.h drivers.xlib
  ```

備考

- ディレクトリを指定した場合、トップレベルで `.xlib` を検索し、ちょうど1つでないとエラーになります。
- `.xlib` 内で参照されるファイルは `.xlib` の場所を基準に検証され、存在しないファイルや未対応拡張子があるとコマンドはエラーで失敗します。

必要であれば、エラーメッセージの例や exit code の仕様を `README_js.md` に追記できます。ご希望があれば追加します。
