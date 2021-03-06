use std::fs::File;
use std::io::prelude::*;
use std::collections::{VecDeque, HashMap, HashSet};

use structopt::StructOpt;
use alpm::{SigLevel, Package};
use anyhow::{Context, Result, Error};
use petgraph::graph::DiGraph;
use petgraph::dot::{Dot, Config};
use petgraph::visit::Bfs;


const ROOT_DIR: &str = "/";
const DB_PATH: &str = "/var/lib/pacman/";

fn find_package_anywhere<'a>(pkgname: &str, pacman: &'a alpm::Alpm) -> Result<Package<'a>, alpm::Error> {

    let dbs = pacman.syncdbs();
    for db in dbs {
        let maybe_pkg = db.pkg(pkgname);
        if maybe_pkg.is_ok() {
            return maybe_pkg;
        }
    };
    Err(alpm::Error::PkgNotFound)
}

fn get_reverse_deps_map(pacman: &alpm::Alpm) -> Result<HashMap<String, Vec<String>>, Error> {
    let mut reverse_deps: HashMap<String, Vec<String>> = HashMap::new();
    let dbs = pacman.syncdbs();

    for db in dbs {
        for pkg in db.pkgs().context(format!("Unable to get packages from sync db {}", db.name()))? {
            for dep in pkg.depends() {
                reverse_deps.entry(dep.name().to_string())
                    .and_modify(|e| e.push(pkg.base().unwrap_or_else(|| pkg.name()).to_string()))
                    .or_insert_with(|| vec![pkg.base().unwrap_or_else(|| pkg.name()).to_string()]);
            }

            for dep in pkg.makedepends() {
                reverse_deps.entry(dep.name().to_string())
                    .and_modify(|e| e.push(pkg.base().unwrap_or_else(|| pkg.name()).to_string()))
                    .or_insert_with(|| vec![pkg.base().unwrap_or_else(|| pkg.name()).to_string()]);
            }
        }
    }

    Ok(reverse_deps)
}

#[derive(Debug, StructOpt)]
#[structopt(name = "rebuilder", about, author)]
struct Args {
    /// List of input packages
    #[structopt(min_values = 1, required = true)]
    pkgnames: Vec<String>,

    /// The path to the pacman database, default ( /var/lib/pacman )
    #[structopt(long)]
    dbpath: Option<String>,

    /// Write a dotfile into the given file
    #[structopt(short, long)]
    dotfile: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::from_args();
    let pkgnames = args.pkgnames;

    let pacman = match args.dbpath {
        Some(path) => {
            alpm::Alpm::new(ROOT_DIR, &path).context("could not initialise pacman db from dbpath")?
        }
        None => alpm::Alpm::new(ROOT_DIR, DB_PATH).context("could not initialise pacman db")?
    };

    let _core = pacman.register_syncdb("core", SigLevel::DATABASE_OPTIONAL);
    let _extra = pacman.register_syncdb("extra", SigLevel::DATABASE_OPTIONAL);
    let _community = pacman.register_syncdb("community", SigLevel::DATABASE_OPTIONAL);
    let _multilib = pacman.register_syncdb("multilib", SigLevel::DATABASE_OPTIONAL);
    let reverse_deps_map = get_reverse_deps_map(&pacman)?;

    for pkg in &pkgnames {
        find_package_anywhere(&pkg, &pacman)?;
    }

    let mut graph = DiGraph::<&str, u16>::new();

    let mut to_visit = VecDeque::new();
    let mut to_build = HashSet::new();
    to_visit.extend(pkgnames.iter().map(|x| x.as_str()));

    let mut cache_node = HashMap::new(); 

    while !to_visit.is_empty() {
        let pkg = if let Some(pkg) = to_visit.pop_front() {
            pkg
        } else {
            break;
        };

        let root = cache_node.entry(pkg).or_insert_with(|| graph.add_node(pkg)).clone();

        if let Some(rev_deps_for_pkg) = reverse_deps_map.get(pkg) {
            if to_build.get(&pkg.to_string()).is_none() {
                to_visit.extend(rev_deps_for_pkg.iter().map(|x| x.as_str()));
            }

            for rev_dep in rev_deps_for_pkg {
                let depnode = cache_node.entry(rev_dep).or_insert_with(|| graph.add_node(rev_dep)).clone();
                if !graph.contains_edge(root, depnode) {
                    graph.add_edge(root, depnode, 1);
                }
            }
            to_build.extend(rev_deps_for_pkg);
        };
    }

    for pkg in &pkgnames {
        if let Some(pkgname) = cache_node.get(pkg.as_str()) {
            let mut bfs = Bfs::new(&graph, *pkgname);

            while let Some(nx) = bfs.next(&graph) {
                let node = graph[nx];
                print!("{} ", node);
            }
            println!();
        }
    }

    if let Some(filename) = args.dotfile {
        let dotgraph = Dot::with_config(&graph, &[Config::EdgeNoLabel]);
        let mut file = File::create(filename)?;
        file.write_all(dotgraph.to_string().as_bytes())?;
    }

    Ok(())
}
