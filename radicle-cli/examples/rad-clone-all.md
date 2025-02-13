<!-- TODO: Currently, `rad clone`, even with `--scope all` will not fetch all remotes -->
<!-- We have to issue a separate `rad sync --fetch` -->

```
$ rad clone rad:z42hL2jL4XNk6K8oHQaSWfMgCL7ji --scope all
✓ Tracking relationship established for rad:z42hL2jL4XNk6K8oHQaSWfMgCL7ji with scope 'all'
✓ Fetching rad:z42hL2jL4XNk6K8oHQaSWfMgCL7ji from z6MknSL…StBU8Vi..
✓ Forking under z6Mkux1…nVhib7Z..
✓ Creating checkout in ./heartwood..
✓ Remote alice@z6MknSLrJoTcukLrE435hVNQT4JUhbvWLX4kUzqkEStBU8Vi added
✓ Remote-tracking branch alice@z6MknSLrJoTcukLrE435hVNQT4JUhbvWLX4kUzqkEStBU8Vi/master created for z6MknSL…StBU8Vi
✓ Repository successfully cloned under [..]/heartwood/
```

We can now have a look at the new working copy that was created from the cloned
repository:

```
$ cd heartwood
$ cat README
Hello World!
```

```
$ rad sync --fetch
✓ Fetching rad:z42hL2jL4XNk6K8oHQaSWfMgCL7ji from z6MknSL…StBU8Vi..
✓ Fetched repository from 1 seed(s)
```

Let's check that we have all the namespaces in storage:

```
$ rad inspect --refs
.
|-- z6MknSLrJoTcukLrE435hVNQT4JUhbvWLX4kUzqkEStBU8Vi
|   `-- refs
|       |-- heads
|       |   `-- master
|       `-- rad
|           |-- id
|           `-- sigrefs
|-- z6Mkt67GdsW7715MEfRuP4pSZxJRJh6kj6Y48WRqVv4N1tRk
|   `-- refs
|       |-- heads
|       |   `-- master
|       `-- rad
|           |-- id
|           `-- sigrefs
`-- z6Mkux1aUQD2voWWukVb5nNUR7thrHveQG4pDQua8nVhib7Z
    `-- refs
        |-- heads
        |   `-- master
        `-- rad
            |-- id
            `-- sigrefs
```

We can then setup a git remote for `bob`:

```
$ rad remote add z6Mkt67GdsW7715MEfRuP4pSZxJRJh6kj6Y48WRqVv4N1tRk --name bob
✓ Remote bob added
✓ Remote-tracking branch bob/master created for z6Mkt67…v4N1tRk
```

And fetch his refs:

```
$ git fetch --all
Fetching rad
Fetching alice@z6MknSLrJoTcukLrE435hVNQT4JUhbvWLX4kUzqkEStBU8Vi
Fetching bob
$ git branch --remotes
  alice@z6MknSLrJoTcukLrE435hVNQT4JUhbvWLX4kUzqkEStBU8Vi/master
  bob/master
  rad/master
```
