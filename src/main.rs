use ansi_term::Colour;
use ansi_term::Style;
use clap::Parser;
use postgres::{Client, NoTls};
use serde::Deserialize;
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::File;
use std::fs::{self, DirEntry};
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;

/// Program to sync function and RLS defintions defined declaratively to a database
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    db_url: String,

    #[arg(long, default_value = "./db_sync.toml")]
    config_path: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    functions: SyncConfig,
    rls_policies: SyncConfig,
}

#[derive(Deserialize)]
struct SyncConfig {
    dir: PathBuf,
    schemas: Vec<String>,
}

#[derive(Debug)]
enum Error {
    IOError(io::Error),
    PostgresError(postgres::Error),
    JsonError(serde_json::Error),
    PgQueryError(pg_query::Error),
    ExecutionError(postgres::Error, PathBuf, String),
    FunctionStatementError(statement::Error, PathBuf, String, Vec<String>),
    RlsStatementError(statement::Error, PathBuf, String, Vec<String>),
    PathDirError(PathBuf),
    PathFileError(PathBuf),
    ConfigError(toml::de::Error),
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let config_str = fs::read_to_string(&args.config_path)
        .map_err(|_| Error::PathFileError(args.config_path))?;
    let config: Config = toml::from_str(&config_str).map_err(|e| Error::ConfigError(e))?;

    if !config.rls_policies.dir.is_dir() {
        return Err(Error::PathDirError(config.rls_policies.dir).into());
    }

    if !config.functions.dir.is_dir() {
        return Err(Error::PathDirError(config.functions.dir).into());
    }

    print!("Syncing database functions and RLS policies... ");

    let mut client = Client::connect(&args.db_url, NoTls)?;
    let mut transaction = client.transaction()?;

    let mut function_statements = get_files_in_dir(&config.functions.dir)?
        .into_iter()
        .map(|dir_entry| {
            let path = dir_entry.path();
            read_file(&path)
                .map_err(|e| Error::IOError(e))
                .and_then(|contents| {
                    str_to_function_statements(&contents, path, &config.functions.schemas)
                })
        })
        .collect::<Result<Vec<(PathBuf, Vec<String>)>, Error>>()?;

    let types_file_name = OsString::from("types.sql");
    function_statements.sort_by(|a, b| {
        let is_a_types = a.0.file_name() == Some(&types_file_name);
        let is_b_types = b.0.file_name() == Some(&types_file_name);
        if is_a_types && is_b_types {
            Ordering::Equal
        } else if is_a_types {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });

    let rls_statements = get_files_in_dir(&config.rls_policies.dir)?
        .into_iter()
        .map(|dir_entry| {
            let path = dir_entry.path();
            read_file(&path)
                .map_err(|e| Error::IOError(e))
                .and_then(|contents| {
                    str_to_rls_statements(&contents, path, &config.rls_policies.schemas)
                })
        })
        .collect::<Result<Vec<(PathBuf, Vec<String>)>, Error>>()?;

    let policies_to_delete = transaction.query(
        "
                SELECT schemaname, tablename, policyname
                FROM pg_policies
                WHERE schemaname = ANY($1)
                ORDER BY schemaname, tablename, policyname;
            ",
        &[&config.rls_policies.schemas],
    )?;
    for row in policies_to_delete {
        let schema_name: String = row.get(0);
        let table_name: String = row.get(1);
        let policy_name: String = row.get(2);
        let statement = format!(
            "DROP POLICY \"{}\" ON \"{}\".\"{}\"",
            &policy_name, &schema_name, &table_name
        );
        transaction.execute(&statement, &[])?;
    }

    let functions_to_delete = transaction.query(
        "
                SELECT
                    n.nspname AS function_schema,
                    p.proname AS function_name,
                    pg_get_function_identity_arguments(p.oid) AS function_arguments
                FROM pg_proc p
                    JOIN pg_namespace n ON p.pronamespace = n.oid
                    JOIN pg_language l ON p.prolang = l.oid
                    JOIN pg_type t ON t.oid = p.prorettype 
                WHERE l.lanname <> 'internal'
                  AND n.nspname = ANY($1)
                  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                  AND t.typname <> 'trigger'
                ORDER BY function_schema, function_name;
            ",
        &[&config.functions.schemas],
    )?;
    for row in functions_to_delete {
        let schema_name: String = row.get(0);
        let table_name: String = row.get(1);
        let args: String = row.get(2);
        let statement = format!(
            "DROP FUNCTION \"{}\".\"{}\"({}) CASCADE",
            &schema_name, &table_name, &args
        );
        transaction.execute(&statement, &[])?;
    }
    let types_to_delete = transaction.query(
            "
                SELECT n.nspname as schema, t.typname as type 
                FROM pg_type t 
                    JOIN   pg_catalog.pg_namespace n ON n.oid = t.typnamespace 
                WHERE (t.typrelid = 0 OR (SELECT c.relkind = 'c' FROM pg_catalog.pg_class c WHERE c.oid = t.typrelid)) 
                  AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_type el WHERE el.oid = t.typelem AND el.typarray = t.oid)
                  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                  AND n.nspname = ANY($1)
            ",
            &[&config.functions.schemas],
        )?;
    for row in types_to_delete {
        let schema_name: String = row.get(0);
        let type_name: String = row.get(1);
        let statement = format!("DROP TYPE \"{}\".\"{}\" CASCADE", &schema_name, &type_name);
        transaction.execute(&statement, &[])?;
    }

    let _ = transaction.query("SET check_function_bodies = FALSE;", &[])?;
    apply_statements_in_transaction(&mut transaction, function_statements)?;

    apply_statements_in_transaction(&mut transaction, rls_statements)?;

    println!("{}", Colour::Green.paint("Done"));
    transaction.commit()?;

    Ok(())
}

fn str_to_function_statements(
    contents: &String,
    path: PathBuf,
    valid_schemas: &Vec<String>,
) -> Result<(PathBuf, Vec<String>), Error> {
    pg_query::split_with_parser(&contents)
        .map_err(|e| Error::PgQueryError(e))
        .and_then(|statements| {
            statements
                .into_iter()
                .map(|statement| {
                    pg_query::parse(statement)
                        .map_err(|e| Error::PgQueryError(e))
                        .and_then(|result| {
                            let node_ref = result.protobuf.nodes()[0].0;
                            statement::validate_function_schema(&node_ref, valid_schemas)
                                .map(|()| statement.to_owned())
                                .map_err(|e| {
                                    Error::FunctionStatementError(
                                        e,
                                        path.clone(),
                                        statement.to_owned(),
                                        valid_schemas.clone(),
                                    )
                                })
                        })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .map(|parse_result| (path, parse_result))
}

fn str_to_rls_statements(
    contents: &String,
    path: PathBuf,
    valid_schemas: &Vec<String>,
) -> Result<(PathBuf, Vec<String>), Error> {
    pg_query::split_with_parser(&contents)
        .map_err(|e| Error::PgQueryError(e))
        .and_then(|statements| {
            statements
                .into_iter()
                .map(|statement| {
                    pg_query::parse(statement)
                        .map_err(|e| Error::PgQueryError(e))
                        .and_then(|result| {
                            let node_ref = result.protobuf.nodes()[0].0;
                            statement::validate_rls_schema(&node_ref, valid_schemas)
                                .map(|()| statement.to_owned())
                                .map_err(|e| {
                                    Error::RlsStatementError(
                                        e,
                                        path.clone(),
                                        statement.to_owned(),
                                        valid_schemas.clone(),
                                    )
                                })
                        })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .map(|parse_result| (path, parse_result))
}

fn apply_statements_in_transaction(
    transaction: &mut postgres::Transaction,
    file_contents: Vec<(PathBuf, Vec<String>)>,
) -> Result<(), Error> {
    let execution_result: Result<(), (PathBuf, String, postgres::Error)> =
        sequence_ignore_operations(&file_contents, |(path, statements)| {
            sequence_ignore_operations(&statements, |raw| {
                transaction
                    .batch_execute(&raw)
                    .map(|_| ())
                    .map_err(|e| (path.clone(), raw.clone(), e))
            })
        });

    match execution_result {
        Ok(()) => Ok(()),
        Err((path_buf, raw_statement, error)) => {
            Err(Error::ExecutionError(error, path_buf, raw_statement))
        }
    }
}

fn sequence_ignore_operations<V, E, F: FnMut(&V) -> Result<(), E>>(
    vec: &Vec<V>,
    mut closure: F,
) -> Result<(), E> {
    vec.into_iter().fold(Ok(()), |acc, value| match acc {
        Ok(()) => closure(value),
        Err(_) => acc,
    })
}

fn read_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn get_files_in_dir(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut files = Vec::new();
    get_files_in_dir_help(dir, &mut files)?;
    Ok(files)
}

fn get_files_in_dir_help(dir: &Path, files: &mut Vec<DirEntry>) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                get_files_in_dir_help(&path, files)?;
            } else {
                files.push(entry);
            }
        }
    }
    Ok(())
}

mod statement {
    use pg_query::protobuf::RangeVar;
    use pg_query::{Node, NodeRef};

    #[derive(Debug)]
    pub enum Error {
        NoSchema,
        InvalidSchema(String),
        InvalidStatement,
    }

    fn validate_name(nodes: &Vec<Node>, schemas: &Vec<String>) -> Result<(), Error> {
        if let [schema, name] = nodes.as_slice() {
            match (&schema.node, &name.node) {
                (Some(pg_query::NodeEnum::String(schema)), Some(_)) => {
                    if schemas.into_iter().any(|s| *s == *schema.sval) {
                        Ok(())
                    } else {
                        Err(Error::InvalidSchema(schema.sval.clone()))
                    }
                }
                _ => Err(Error::NoSchema),
            }
        } else {
            Err(Error::NoSchema)
        }
    }

    fn validate_range_var(range_var: &RangeVar, schemas: &Vec<String>) -> Result<(), Error> {
        let schema_name = &range_var.schemaname;
        if schemas.into_iter().any(|s| s == schema_name) {
            Ok(())
        } else {
            Err(Error::InvalidSchema(schema_name.clone()))
        }
    }

    pub fn validate_function_schema(
        node_ref: &NodeRef,
        schemas: &Vec<String>,
    ) -> Result<(), Error> {
        match node_ref {
            NodeRef::CreateFunctionStmt(v) => validate_name(&v.funcname, schemas),
            NodeRef::CompositeTypeStmt(v) => match &v.typevar {
                Some(v) => validate_range_var(&v, schemas),
                None => Err(Error::NoSchema),
            },
            NodeRef::CreateDomainStmt(v) => validate_name(&v.domainname, schemas),
            NodeRef::CreateEnumStmt(v) => validate_name(&v.type_name, schemas),
            NodeRef::CreateRangeStmt(v) => validate_name(&v.type_name, schemas),
            _ => Err(Error::InvalidStatement),
        }
    }

    pub const FUNCTION_STATEMENTS: [&'static str; 5] = [
        "CREATE FUNCTION",
        "CREATE TYPE",
        "CREATE DOMAIN",
        "CREATE ENUM",
        "CREATE RANGE",
    ];

    pub fn validate_rls_schema(node_ref: &NodeRef, schemas: &Vec<String>) -> Result<(), Error> {
        match node_ref {
            NodeRef::CreatePolicyStmt(v) => match &v.table {
                Some(v) => validate_range_var(&v, schemas),
                None => Err(Error::NoSchema),
            },
            _ => Err(Error::InvalidStatement),
        }
    }

    pub const RLS_STATEMENTS: [&'static str; 1] = ["CREATE POLICY"];
}

// Error impls

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IOError(e)
    }
}

impl From<postgres::Error> for Error {
    fn from(e: postgres::Error) -> Self {
        Error::PostgresError(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::JsonError(e)
    }
}

impl From<pg_query::Error> for Error {
    fn from(e: pg_query::Error) -> Self {
        Error::PgQueryError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        println!("{}", Colour::Red.paint("Failed"));

        match self {
            Error::ExecutionError(error, path, raw_statement) => {
                write!(
                    f,
                    "Invalid statement in file {}:\n\n{}\n\nPostgres error was:\n\n{}\n\nPolicies and functions dropped/applied before the error been rolled back and the database has not been changed.",
                    Style::new().bold().paint(path.to_str().unwrap_or("")),
                    Colour::Blue.paint(raw_statement.trim()),
                    Colour::Red.paint(error.to_string()),
                )
            }
            Error::FunctionStatementError(statement_error, path, raw_statement, valid_schemas) => {
                let sub_err_str = match statement_error {
                    statement::Error::NoSchema => {
                        "You must schema specify a schema for this statement".to_owned()
                    }
                    statement::Error::InvalidSchema(schema) => {
                        format!(
                            "Schema \"{}\" isn't valid. Expected schema to be one of:\n\n{}",
                            schema,
                            format_as_bullet_list(valid_schemas)
                        )
                    }
                    statement::Error::InvalidStatement => {
                        format!(
                            "Expected statement to be one of:\n\n{}",
                            format_as_bullet_list(
                                &statement::FUNCTION_STATEMENTS
                                    .into_iter()
                                    .map(|v| v.to_owned())
                                    .collect()
                            )
                        )
                    }
                };

                write!(
                    f,
                    "\
                    Invalid statement type in function file {}:\n\n\
                    {}\
                    \n\n\
                    Problem was:\n\n\
                    {}\
                    \n\n\
                    Policies and functions dropped/applied before the error been rolled back and the database has not been changed.\
                    ",
                    Style::new().bold().paint(path.to_str().unwrap_or("")),
                    Colour::Blue.paint(raw_statement.trim()),
                    Colour::Red.paint(sub_err_str),
                )
            }
            Error::RlsStatementError(statement_error, path, raw_statement, valid_schemas) => {
                let sub_err_str = match statement_error {
                    statement::Error::NoSchema => {
                        "You must schema specify a schema for this statement".to_owned()
                    }
                    statement::Error::InvalidSchema(schema) => {
                        format!(
                            "Schema \"{}\" isn't valid. Expected schema to be one of:\n\n{}",
                            schema,
                            format_as_bullet_list(valid_schemas)
                        )
                    }
                    statement::Error::InvalidStatement => {
                        format!(
                            "Expected statement to be one of:\n\n{}",
                            format_as_bullet_list(
                                &statement::RLS_STATEMENTS
                                    .into_iter()
                                    .map(|v| v.to_owned())
                                    .collect()
                            )
                        )
                    }
                };

                write!(
                    f,
                    "\
                    Invalid statement type in RLS policy file {}:\n\n\
                    {}\
                    \n\n\
                    Problem was:\n\n\
                    {}\
                    \n\n\
                    Policies and functions dropped/applied before the error been rolled back and the database has not been changed.\
                    ",
                    Style::new().bold().paint(path.to_str().unwrap_or("")),
                    Colour::Blue.paint(raw_statement.trim()),
                    Colour::Red.paint(sub_err_str),
                )
            }
            Error::PathDirError(path) => {
                write!(
                    f,
                    "Path {} does not exist or is not a directory",
                    Colour::Red.bold().paint(path.to_str().unwrap_or(""))
                )
            }
            Error::PathFileError(path) => {
                write!(
                    f,
                    "Path {} does not exist",
                    Colour::Red.bold().paint(path.to_str().unwrap_or(""))
                )
            }
            Error::ConfigError(error) => {
                write!(f, "Invalid config: {:?}", error.message())
            }
            e => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {}

fn format_as_bullet_list(v: &Vec<String>) -> String {
    v.into_iter()
        .map(|v| format!("    - {}", v))
        .collect::<Vec<String>>()
        .join("\n")
}
