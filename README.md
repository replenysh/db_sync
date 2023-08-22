# DB Sync

This is a tool used to sync functions and RLS policies to a database based on definitinos in a file tree.
We want to do this so we can manage these entities in a declarative way and take advantage of version control in peer reviews.
There are some limitations to this tool, so please read this document carefully to understand the semantics of 
this tool.

## Overview

This tool takes a directory structure and syncs its definitions to a database. Given a file tree like:

```
functions/
├── schema_a
│   ├── types.sql
│   └── function_1.sql
└── schema_b
    └── function_2.sql

rls_policies/
├── schema_c
│   └── table_1.sql
└── schema_d
    └── table_2.sql
```

And a config file like:
```json
{
  "functions": {
    "dir": "./functions",
    "schemas": ["schema_a", "schema_b"]
  },
  "rls_policies": {
    "dir": "./rls_policies",
    "schemas": ["schema_c", "schema_d"]
  }
}

```

Running this tool will, in order:

1. Drop all RLS policies for all tables in schemas `c`, and `d`
2. Drop all functions and types in  schemas `a`, and `b`
3. Run all files named `types.sql` in `functions/`
4. Run all other files in `functions/`
5. Run all policy statements in `rls_policies/`

All steps are run in the same postgres transaction, so if anything fails all changes are rolledback and the database is untouched.

### Config & filetree

This tool needs a config file, defined here as `config.json`.

This config specifies the directories to use for functions & RLS policies (currently defined as `functions/` and `rls_policies/` respectively).
Additionally, it specifies the schemas to work with for both functions and policies. This is an important detail!
If you tried to define `CREATE FUNCTION schema_d.new_function` in the file `functions/schema_a/new_function`.
This tool will error, because `schema_d` isn't in `config.functions.schemas`. 

The file tree doesn't actually matter! You could, in theory have a file tree like:
```
functions/
├── schema_a_function_1.sql
└── schema_b_function_2.sql
```

So long as the files only use the schemas defined in `config.functions.schemas`, this tool will work.

That said, we recommend using the 1st filetree structure mentioned in this document as it's the most organized and works nicely with git diffs. 

#### `types.sql`

You can define files called `types.sql` in your `functions/` directory. These will run before the rest of the files, so it's a great place to 
create types that the multiple other functions use.

> Other than the `types.sql` files, there is no gaurenteed order the rest of the functions files will be run. If you have multiple `types.sql` files in different directories, the order that the `types` files will be run in is also not gaurenteed. However, all `types` files will be run before any othe file.

#### Functions bodies

Before the functions are run, we the command `SET check_function_bodies = FALSE;`. This disables the validation of bodies of functions, allowing you to reference other functions in other files without us having to do tons of cyclical dependency work. However, this means that some errors will not be caught until the functions are run. So always test you code! And write pgtap tests too!

### Statements in the files

All statements across all files should only be `CREATE` statements, because we drop all existing functions/types/policies before running the definitions. 

In `functions/`, only the following statements are allow:
- `CREATE FUNCTION`
- `CREATE TYPE`
- `CREATE DOMAIN`
- `CREATE ENUM`
- `CREATE RANGE`

In `rls_policies/`, only the following statements are allow:
- `CREATE POLICY`

This tool will error if there is an unallowed statement. 

### Functions Limitations

You might've notices in our `config`, we include a bunch of schemas in our `rls_policies/` directiry, but only
a few in our `functions/` directory. This is becasue we can't manage all functions with this tool.
Namely, we can't manage Postgraphile virtual column functions ([docs](https://www.graphile.org/postgraphile/computed-columns/))
or any functions that a table uses in it's generated columns or it's virtual columns functions. Additionally, since 
we clear out the function schemas and re-create the functions every time, it's safer to only use specific, dedicate function schemas with this tool. 

#### Postgraphile

Due to the limitations mentioned above, we have 2 main schemas we create functions with:

1. `functions_public`
2. `functions_private`

As you can guess, functions in `public` are exposed via graphql and `private` onces are not.

Overtime, we should migrate all functions that we can into these schemas for easier management.

> Note: We can do this progressively, meaining as we update functions, we can drop them from their old locations and use them in new locations. Additionally we can use the `postgraphile.tags` fileto preserve names of functions in the public API so we don't introduce breaking changes

> Note 2: We also include the `go` schema, as that's a schema that's used only by the `replenysh-go` app and has a super clear scope. (It only has 1 function it in, nothing else in the schema) 
