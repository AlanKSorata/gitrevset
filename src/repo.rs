use crate::ast::Expr;
use crate::ast::ParseToExpr;
use crate::EvalContext;
use crate::Result;
use dag::namedag::MemNameDag;
use dag::DagAlgorithm;
use dag::Set;
use dag::Vertex;
use gitdag::dag;
use gitdag::git2;
use gitdag::GitDag;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Repo with extra states to support revset queries.
pub struct Repo {
    git_repo: Box<dyn AsRef<git2::Repository>>,
    dag: GitDag,
    cached_sets: Mutex<HashMap<&'static str, Set>>,
    cached_mutation_dag: OnceCell<MemNameDag>,
    cached_eval_context: OnceCell<EvalContext>,
}

impl Repo {
    /// Open an existing repo. Build indexes on demand.
    pub fn open_from_env() -> Result<Self> {
        let repo = git2::Repository::open_from_env()?;
        Self::open_from_repo(Box::new(repo))
    }

    /// Open an existing repo previously opened by libgit2.
    /// Build commit graph indexes on demand.
    pub fn open_from_repo(git_repo: impl AsRef<git2::Repository> + 'static) -> Result<Self> {
        let git_repo_ref = git_repo.as_ref();
        let dag_path = git_repo_ref.path().join("dag");
        let main_branch_name = guess_main_branch_name(git_repo_ref);
        let dag = GitDag::open_git_repo(git_repo_ref, &dag_path, &main_branch_name)?;
        let cached_sets = Default::default();
        let cached_mutation_dag = Default::default();
        let cached_eval_context = Default::default();
        let result = Repo {
            git_repo: Box::new(git_repo),
            dag,
            cached_sets,
            cached_mutation_dag,
            cached_eval_context,
        };

        Ok(result)
    }

    /// Open a git repository from a local path.
    ///
    /// Unlike [`open_from_env`](Self::open_from_env) which relies on the
    /// current working directory or `GIT_DIR`, this method opens a repo
    /// at the given path explicitly.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let git_repo = git2::Repository::open(path.as_ref())?;
        Self::open_from_repo(Box::new(git_repo))
    }

    /// Clone a remote repository and open it with the revset engine.
    ///
    /// This clones the remote `url` to the local `path`, builds the
    /// commit graph index, and returns a ready-to-use `Repo`.
    ///
    /// ```no_run
    /// # fn main() -> gitrevset::Result<()> {
    /// let repo = gitrevset::Repo::clone(
    ///     "https://github.com/rust-lang/rust.git",
    ///     "/tmp/rust-clone",
    /// )?;
    /// let set = repo.revs("head()")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone(url: &str, path: impl AsRef<Path>) -> Result<Self> {
        // libgit2 does not read HTTP_PROXY / HTTPS_PROXY env vars,
        // so forward them explicitly if set.
        let proxy_url = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
            .ok();

        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, allowed| {
            if allowed.contains(git2::CredentialType::SSH_KEY) {
                return git2::Cred::ssh_key_from_agent(
                    username_from_url.unwrap_or("git"),
                );
            }
            if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                // Return a no-credential error so libgit2 continues
                // (some anonymous servers need this to proceed)
            }
            Err(git2::Error::from_str("no credentials available"))
        });

        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);
        if let Some(ref proxy_url) = proxy_url {
            let mut proxy = git2::ProxyOptions::new();
            proxy.url(proxy_url);
            fetch_opts.proxy_options(proxy);
        }

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_opts);
        let git_repo = builder.clone(url, path.as_ref())?;
        Self::open_from_repo(Box::new(git_repo))
    }

    /// Fetch from a remote and refresh the commit graph index.
    ///
    /// After fetching, cached sets are cleared and the DAG index is
    /// rebuilt to include newly fetched commits.
    ///
    /// ```no_run
    /// # fn main() -> gitrevset::Result<()> {
    /// let mut repo = gitrevset::Repo::open_from_env()?;
    /// repo.fetch("origin")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn fetch(&mut self, remote_name: &str) -> Result<()> {
        {
            let mut remote = self.git_repo().find_remote(remote_name)?;
            remote.fetch(&[] as &[&str], None, None)?;
        }
        // remote is dropped, releasing the immutable borrow on self
        self.refresh_dag()
    }

    /// Rebuild the DAG index and invalidate caches after the git repo
    /// has been changed (e.g., via `fetch`).
    fn refresh_dag(&mut self) -> Result<()> {
        let git_repo_path = self.git_repo().path().to_owned();
        let main_branch = guess_main_branch_name(self.git_repo());
        // Re-open the repo temporarily to rebuild the DAG index,
        // avoiding a concurrent borrow with self.dag.
        let temp_repo = git2::Repository::open(&git_repo_path)?;
        let dag_path = git_repo_path.join("dag");
        self.dag = GitDag::open_git_repo(&temp_repo, &dag_path, &main_branch)?;
        self.cached_sets.lock().unwrap().clear();
        self.cached_mutation_dag = OnceCell::new();
        self.cached_eval_context = OnceCell::new();
        Ok(())
    }

    /// Evaluate the expression. Return the resulting set.
    /// User-defined aliases are ignored.
    pub fn revs(&self, ast: impl ParseToExpr) -> Result<Set> {
        self.revs_with_context(ast, &Default::default())
    }

    /// Evaluate the expression. Return the resulting set.
    /// User-defined aliases are respected.
    ///
    /// To define aliases, add a `[revsetalias]` section like:
    ///
    /// ```plain,ignore
    /// [revsetalias]
    /// # f(x) can be used, and will be expended to ancestor(x) + x.
    /// f = ancestor($1) + $1
    /// ```
    pub fn anyrevs(&self, ast: impl ParseToExpr) -> Result<Set> {
        self.revs_with_context(ast, self.eval_context_from_config()?)
    }

    /// Evaluate the expression with the given context.
    /// Return the resulting set.
    pub fn revs_with_context(&self, ast: impl ParseToExpr, ctx: &EvalContext) -> Result<Set> {
        let ast = ast.parse_to_expr()?;
        crate::eval::eval(self, &ast, ctx)
    }

    /// Obtains read-only `dag` reference.
    pub fn dag(&self) -> &GitDag {
        &self.dag
    }

    /// Obtains read-only `git2::Repository` reference.
    pub fn git_repo(&self) -> &git2::Repository {
        self.git_repo.as_ref().as_ref()
    }

    /// Returns a `EvalContext` that contains user-defined alias
    /// in the `[revsetalias]` config section.
    pub fn eval_context_from_config(&self) -> Result<&EvalContext> {
        self.cached_eval_context
            .get_or_try_init(|| parse_eval_context(self.git_repo()))
    }

    pub(crate) fn cached_set(
        &self,
        name: &'static str,
        func: impl Fn(&Repo) -> Result<Set>,
    ) -> Result<Set> {
        if let Some(set) = self.cached_sets.lock().unwrap().get(name) {
            return Ok(set.clone());
        }
        match func(self) {
            Err(e) => Err(e),
            Ok(set) => {
                self.cached_sets.lock().unwrap().insert(name, set.clone());
                Ok(set)
            }
        }
    }

    pub(crate) fn to_set(&self, iter: impl IntoIterator<Item = Vertex>) -> Result<Set> {
        Ok(self.dag.sort(&Set::from_static_names(iter.into_iter()))?)
    }

    pub(crate) fn mutation_dag(&self) -> Result<&MemNameDag> {
        self.cached_mutation_dag
            .get_or_try_init(|| crate::mutation::infer_mutation_from_reflog(self))
    }
}

fn guess_main_branch_name(repo: &git2::Repository) -> String {
    if let Ok(config) = repo.config() {
        if let Ok(s) = config.get_str("revs.main-branch") {
            return s.to_string();
        }
    }
    let candidates = [
        "refs/remotes/origin/master",
        "refs/remotes/origin/main",
        "refs/remotes/upstream/master",
        "refs/remotes/upstream/main",
    ];
    candidates
        .iter()
        .cloned()
        .find(|name| repo.refname_to_id(name).is_ok())
        .unwrap_or(candidates[0])
        .to_string()
}

fn parse_eval_context(repo: &git2::Repository) -> Result<EvalContext> {
    let mut result = EvalContext::default();
    let config = repo.config()?;
    for entry in &config.entries(Some("revsetalias.*"))? {
        let entry = entry?;
        if let (Some(name), Some(value)) = (entry.name(), entry.value()) {
            if let Some(name) = name.get("revsetalias.".len()..) {
                if let Ok(ast) = value.parse_to_expr() {
                    let func = move |_name: &str,
                                     repo: &Repo,
                                     args: &[Expr],
                                     ctx: &EvalContext|
                          -> Result<Set> {
                        // Replace arguments in ast, ex. $1 -> args[0], ...
                        let mut ast = ast.clone();
                        for (i, arg) in args.iter().enumerate() {
                            ast.replace(&format!("${}", i + 1), arg);
                        }
                        repo.revs_with_context(ast, ctx)
                    };
                    result.fns.insert(name.to_string(), Box::new(func));
                }
            }
        }
    }
    Ok(result)
}
