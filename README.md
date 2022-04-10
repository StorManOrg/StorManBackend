<p align="center">
  <a href="https://github.com/StorageReloaded/Server">
    <img alt="storage-reloaded" width="350"
         src="https://raw.githubusercontent.com/StorageReloaded/Server/master/banner.svg?sanitize=true">
  </a>
</p>

# StoRe Server
![Rust Stable 1.56.1+](https://img.shields.io/badge/Rust%20Stable-1.54%2B-informational)
[![Rust CI](https://github.com/StorageReloaded/Server/actions/workflows/rust.yml/badge.svg)](https://github.com/StorageReloaded/Server/actions/workflows/rust.yml)
[![License](https://img.shields.io/github/license/StorageReloaded/Server)](https://github.com/StorageReloaded/Server/blob/master/LICENSE) 

WIP

## Setup development environment
System:
* [Install Rust](https://rustup.rs)
* MariaDB Server
	* e.g.``docker run -p 3306:3306 --name store-db -e MARIADB_ROOT_PASSWORD=password123 -d mariadb:10.5-focal`` 

VS-Code Extensions:
* [Official Rust Extension](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust)
  * Alternative: [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=matklad.rust-analyzer) (better auto completion)
* [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb) (for debugging)

Workspace:
```shell
$ git clone https://github.com/StorageReloaded/Server.git
$ cd Server/
$ cargo run
```

## Links
[:book: Wiki](https://github.com/StorageReloaded/StoRe/wiki)
|
[:globe_with_meridians: Web](https://github.com/StorageReloaded/Web)
|
[:iphone: Android](https://github.com/StorageReloaded/Android)

## License
Distributed under the **GPL-3.0 License**. See ``LICENSE`` for more information
