use crate::ext::OidExt;
use crate::testrepo::TestRepo;
use crate::Repo;
use gitdag::dag::Set;

#[test]
fn test_revset_functions() {
    let mut repo = TestRepo::new();
    repo.drawdag(
        r#"
    A---B---C---D---E
         \     /
          F---G---H---I
    "#,
    );

    // Basic set operations.
    assert_eq!(
        repo.query("all()"),
        ["I", "H", "E", "D", "G", "F", "C", "B", "A"]
    );
    assert_eq!(repo.query("heads(all())"), ["I", "E"]);
    assert_eq!(repo.query("heads(A:C + G:H)"), ["H", "C"]);
    assert_eq!(repo.query("roots(all())"), ["A"]);
    assert_eq!(repo.query("roots(A:C + G:H)"), ["G", "A"]);
    assert_eq!(repo.query("B:D"), ["D", "G", "F", "C", "B"]);
    assert_eq!(repo.query("A:E - G - (C^ + C)^"), ["E", "D", "F", "C"]);
    assert_eq!(repo.query("!!!(B:D)"), ["I", "H", "E", "A"]);
    assert_eq!(repo.query("::B + G::"), ["I", "H", "E", "D", "G", "B", "A"]);
    assert_eq!(repo.query("H % E"), ["H"]);
    assert_eq!(repo.query("H % C"), ["H", "G", "F"]);
    assert_eq!(repo.query("gca(E+H)"), ["G"]);
    assert_eq!(repo.query("gca(E,H)"), ["G"]);
    assert_eq!(repo.query("first(A:D)"), ["D"]);
    assert_eq!(repo.query("first(B-B,C,D)"), ["C"]);
    assert_eq!(repo.query("first(B-B,C+D)"), ["D"]);
    assert_eq!(repo.query("last(A:D)"), ["A"]);
    assert_eq!(
        repo.query("children(G) | children(A:B)"),
        ["H", "D", "F", "C", "B"]
    );
    assert_eq!(repo.query("head()"), ["I", "E"]);
    assert_eq!(repo.query("desc(C)"), ["C"]);
    assert_eq!(repo.query("author(D)"), ["D"]);
    assert_eq!(repo.query("heads(author(test))"), ["I", "E"]);
    assert_eq!(repo.query("committer(E)"), ["E"]);
    assert_eq!(repo.query("heads(committer(test))"), ["I", "E"]);
    assert_eq!(repo.query("modifies(B)"), ["B"]);

    // date(), committerdate()
    assert_eq!(repo.query(r#"date("0 0")"#), ["B", "A"]);
    assert_eq!(repo.query(r#"date("0 0 to 1 0")"#), ["C", "B", "A"]);
    assert_eq!(
        repo.query(r#"committerdate("before 2 0")"#),
        ["C", "B", "A"]
    );
    assert_eq!(repo.query(r#"committerdate("since 6 0")"#), ["I", "E", "D"]);

    // public(), draft()
    repo.add_ref("refs/heads/master", repo.query_single_oid("E"));
    repo.add_ref("refs/remotes/origin/master", repo.query_single_oid("D"));
    repo.add_ref("refs/remotes/origin/stable", repo.query_single_oid("B"));
    repo.add_ref("refs/tags/v1", repo.query_single_oid("A"));
    repo.add_ref("refs/tags/v2", repo.query_single_oid("B"));

    assert_eq!(repo.query("origin/master"), ["D"]);
    assert_eq!(repo.query("draft()"), ["E", "I", "H"]);
    assert_eq!(repo.query("public()"), ["D", "G", "F", "C", "B", "A"]);
    assert_eq!(repo.query("drafthead()"), ["E", "I"]);
    assert_eq!(repo.query("publichead()"), ["D", "B"]);

    // id(), ref(), tag(), "."
    for name in repo.query("all()") {
        let rev_code = format!("id({})", repo.query_single_oid(&name).to_vertex().to_hex());
        assert_eq!(repo.query(&rev_code), [name.clone()]);
    }
    assert_eq!(
        repo.query("ref()"),
        ["E", "I", "H", "D", "G", "F", "C", "B", "A"]
    );
    assert_eq!(repo.query("ref(origin/master)"), ["D"]);
    assert_eq!(repo.query(r#"ref("remotes/origin/*")"#), ["D", "B"]);
    assert_eq!(repo.query("."), ["E"]);
    assert_eq!(repo.query("tag()"), ["B", "A"]);
    assert_eq!(repo.query("tag(v2)"), ["B"]);
    assert_eq!(repo.query(r#"tag("v*")"#), ["B", "A"]);

    // empty(), present()
    assert!(repo.query("none()").is_empty());
    assert!(repo.query("present(foobar)").is_empty());
    assert_eq!(repo.query("present(master)"), ["E"]);

    // predecessors(), successors()
    repo.amend("refs/heads/H");
    assert_eq!(repo.query("H"), ["H_new"]);
    assert_eq!(repo.query("H_old"), ["H"]);
    assert_eq!(repo.query("predecessors(H)"), ["H_new", "H"]);
    assert_eq!(repo.query("successors(H_old)"), ["H_new", "H"]);
    assert_eq!(repo.query("obsolete()"), ["H"]);

    // apply
    assert_eq!(repo.query("apply($1, .)"), ["E"]);
    assert_eq!(repo.query("apply($1 + $2^, ., B)"), ["E", "A"]);
    assert_eq!(repo.query("apply(apply($1, C) + $1, A)"), ["C", "A"]);
}

#[test]
fn test_revset_alias_config() {
    let mut repo = TestRepo::new();
    repo.drawdag("A--B--C");
    repo.set_config("revsetalias.t", "B");
    repo.set_config("revsetalias.f", "$1^^ + $1");
    repo.set_config("revsetalias.g", "apply(f($1), children($1))");

    assert_eq!(repo.query_with_alias_config("t"), ["B"]);
    assert_eq!(repo.query_with_alias_config("t()"), ["B"]);
    assert_eq!(repo.query_with_alias_config("f(C)"), ["C", "A"]);
    assert_eq!(repo.query_with_alias_config("f(t())"), ["B"]);
    assert_eq!(repo.query_with_alias_config("g(t)"), ["C", "A"]);
}

#[test]
fn test_ext() {
    use crate::ext::OidExt;
    use crate::ext::OidIterExt;
    use crate::ext::SetExt;
    use crate::ext::VertexExt;
    use gitdag::git2::Oid;

    let oid = Oid::zero();
    let v = oid.to_vertex();
    assert_eq!(oid, v.to_oid().unwrap());

    let oid2 = {
        let mut bytes = oid.as_bytes().to_vec();
        bytes[0] ^= 1;
        Oid::from_bytes(&bytes).unwrap()
    };
    let oid_list = [oid, oid2];
    let set = oid_list.to_vec().to_set();
    assert_eq!(
        oid_list.to_vec(),
        set.to_oids()
            .unwrap()
            .collect::<crate::Result<Vec<_>>>()
            .unwrap()
    );
}

#[test]
fn test_ast_macro() {
    use crate::ast;
    let f = |e| format!("{:?}", e);
    assert_eq!(f(ast!("foo")), "foo");
    assert_eq!(f(ast!(parents("foo"))), "parents(foo)");
    assert_eq!(f(ast!(draft())), "draft()");
    assert_eq!(
        f(ast!(union(draft(), public()))),
        "union(draft(), public())"
    );

    let name = "foo";
    assert_eq!(
        f(ast!(union(desc({ name }), author({ "bar" })))),
        "union(desc(foo), author(bar))"
    );

    let set = Set::from_static_names(vec!["A".into(), "B".into()]);
    assert_eq!(f(ast!(parents({ set }))), "parents(<static [A, B]>)")
}

#[test]
fn test_ast_repo() -> crate::Result<()> {
    use crate::ast;
    let mut repo = TestRepo::new();
    repo.drawdag("A-B-C-D");
    repo.add_ref("refs/heads/master", repo.query_single_oid("D"));
    repo.add_ref("refs/remotes/origin/master", repo.query_single_oid("B"));

    let master = "origin/master";
    let stack = repo.revs(ast!(only(".", ref({ master })))).unwrap();
    assert_eq!(repo.desc_set(&stack), ["D", "C"]);
    let head = repo.revs(ast!(heads({ stack }))).unwrap();
    assert_eq!(repo.desc_set(&head), ["D"]);
    Ok(())
}

#[test]
fn test_repo_open() {
    let mut tr = TestRepo::new();
    tr.drawdag("A--B--C");
    tr.add_ref("refs/heads/master", tr.query_single_oid("C"));
    // Open from .git path
    let repo = Repo::open(tr.git_repo().path()).unwrap();
    assert_eq!(
        repo.revs("all()").unwrap().count().unwrap(),
        3
    );
    assert_eq!(repo.revs("master").unwrap().count().unwrap(), 1);
}

#[test]
fn test_repo_clone() {
    // Build a "remote" repo with known commits
    let mut remote = TestRepo::new();
    remote.drawdag("A--B");
    // Create a proper HEAD so the clone works
    remote.add_ref("refs/heads/master", remote.query_single_oid("B"));

    // Clone via local filesystem path
    let url = remote
        .git_repo()
        .path()
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let clone_dir = tempfile::tempdir().unwrap();
    let repo = Repo::clone(&url, clone_dir.path()).unwrap();

    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 2);
    assert_eq!(repo.revs("master").unwrap().count().unwrap(), 1);
}

#[test]
fn test_repo_fetch() {
    // Source repo with one commit
    let mut remote = TestRepo::new();
    remote.drawdag("A");
    remote.add_ref("refs/heads/master", remote.query_single_oid("A"));
    let url = remote
        .git_repo()
        .path()
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Clone
    let clone_dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::clone(&url, clone_dir.path()).unwrap();
    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 1);

    // Add a new commit to the remote
    remote.drawdag("A--B");
    remote.add_ref("refs/heads/master", remote.query_single_oid("B"));

    // Before fetch, clone still sees only 1 commit
    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 1);

    // Fetch and verify new commit appears
    repo.fetch("origin").unwrap();
    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 2);
}

#[test]
fn test_repo_fetch_all_remotes_default() {
    // Test fetch with default refspecs on a multi-commit repo
    let mut remote = TestRepo::new();
    remote.drawdag("A--B--C");
    remote.add_ref("refs/heads/master", remote.query_single_oid("C"));
    let url = remote
        .git_repo()
        .path()
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let clone_dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::clone(&url, clone_dir.path()).unwrap();
    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 3);

    // Remote gets a new branch (D is a new root commit, not related to existing C)
    remote.drawdag("D--E");
    remote.add_ref("refs/heads/feature", remote.query_single_oid("E"));

    repo.fetch("origin").unwrap();
    assert_eq!(repo.revs("all()").unwrap().count().unwrap(), 5);
    // The new branch should also be resolvable as a remote-tracking ref
    assert!(repo.revs("origin/feature").is_ok());
}

#[test]
fn test_repo_real() {
    let clone_dir = tempfile::tempdir().unwrap();

    // Use system git for clone so all user config (credential helpers,
    // proxies, CA bundles) are respected.
    let status = std::process::Command::new("git")
        .args([
            "clone",
            "--quiet",
            "https://github.com/AlanKSorata/code_mig_agent",
            clone_dir.path().to_str().unwrap(),
        ])
        .status()
        .expect("failed to run git clone");
    assert!(status.success(), "git clone failed");

    // Open via gitrevset and run queries against the cloned repo.
    let repo = Repo::open(clone_dir.path()).unwrap();
    let set = repo.revs("head()").unwrap();
    let head_count = set.count().unwrap();
    println!("head() count: {head_count}");
    assert_eq!(head_count, 1, "AlanKSorata/code_mig_agent currently has 1 branch head");

    let all = repo.revs("all()").unwrap();
    let all_count = all.count().unwrap();
    println!("all() count: {all_count}");
    assert_eq!(all_count, 2, "AlanKSorata/code_mig_agent currently has 2 commits total");
}
