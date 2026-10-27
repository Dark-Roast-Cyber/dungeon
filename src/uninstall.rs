//! Explicit, fail-closed teardown of the supported local installation.
use super::*;
use crate::config::Config;
use crate::openshell::Runtime;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

struct Plan {
    config: PathBuf,
    state: PathBuf,
    binary: PathBuf,
    bindings: Vec<Config>,
    gateway_id: Option<String>,
    registration: bool,
    network: bool,
    containers: BTreeSet<String>,
    sandboxes: BTreeSet<String>,
}

fn id(text: &str) -> Result<String> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new("unrecognized Podman container identity"));
    }
    Ok(text.into())
}

fn tree(path: &Path, remaining: &mut usize, device: Option<u64>) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if *remaining == 0
        || metadata.uid() != unsafe { libc::geteuid() }
        || device.is_some_and(|d| d != metadata.dev())
        || !(metadata.is_dir() || metadata.is_file())
        || (metadata.is_file() && metadata.nlink() != 1)
    {
        return Err(Error::new(
            "unsafe or oversized uninstall tree; inspect manually",
        ));
    }
    *remaining -= 1;
    if metadata.is_dir() {
        directory(path, false, false)?;
        for entry in fs::read_dir(path)? {
            tree(&entry?.path(), remaining, Some(metadata.dev()))?;
        }
    }
    Ok(())
}

fn check_tree(path: &Path, allowed: &[&str]) -> Result<()> {
    if !present(path)? {
        return Ok(());
    }
    directory(path, false, true)?;
    for entry in fs::read_dir(path)? {
        if !allowed
            .iter()
            .any(|name| entry.as_ref().is_ok_and(|e| e.file_name() == *name))
        {
            return Err(Error::new(
                "unknown files in Dungeon root; inspect before uninstall",
            ));
        }
    }
    tree(path, &mut 100_000, None)
}

fn check_binary(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("binary parent required"))?;
    if !safe_path(parent, &[]) {
        return Err(Error::new("unsafe uninstall binary directory"));
    }
    if !present(path)? {
        return Ok(());
    }
    directory(parent, false, false)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o022 != 0
    {
        return Err(Error::new(
            "installed dungeon must be an ordinary user-owned binary",
        ));
    }
    let mut installed = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let mut running = fs::File::open(env::current_exe()?)?;
    // Compare bytes without executing a possibly unrelated install or loading it all into memory.
    let mut left = [0; 8192];
    let mut right = [0; 8192];
    loop {
        let a = installed.read(&mut left)?;
        let b = running.read(&mut right)?;
        if a != b || left[..a] != right[..b] {
            return Err(Error::new(
                "installed dungeon differs from this executable; run uninstall with the installed binary",
            ));
        }
        if a == 0 {
            return Ok(());
        }
    }
}

fn network_containers(tools: &Tools) -> Result<BTreeSet<String>> {
    tools
        .podman(
            &[
                "ps",
                "--all",
                "--no-trunc",
                "--filter",
                "network=dungeon-local",
                "--format",
                "{{.ID}}",
            ],
            "uninstall network container inventory failed",
        )?
        .lines()
        .map(id)
        .collect()
}

fn sandbox_ids(tools: &Tools) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for offset in (0..=4096).step_by(100) {
        let text = tools.openshell(
            &["--gateway", GATEWAY, "sandbox", "list", "--all-workspaces", "--ids", "--limit", "100", "--offset", &offset.to_string()],
            "uninstall all-workspace sandbox inventory failed; start a stopped gateway explicitly before uninstall",
        )?;
        let page: Vec<_> = text.lines().collect();
        if page.is_empty() {
            return Ok(ids);
        }
        if page.len() > 100 {
            break;
        }
        for entry in page {
            crate::config::safe_name(entry, "sandbox id")?;
            if !ids.insert(entry.into()) {
                return Err(Error::new("ambiguous uninstall sandbox inventory"));
            }
        }
    }
    Err(Error::new("too many sandboxes to uninstall safely"))
}

fn runtime<'a>(tools: &Tools, binding: &'a Config) -> Result<Runtime<'a>> {
    // Reuse setup's version-checked native tools and environment, not another PATH lookup.
    if tools.openshell.starts_with(&binding.values.project)
        || tools.podman.starts_with(&binding.values.project)
    {
        return Err(Error::new("refusing project-owned teardown tools"));
    }
    let mut environment = tools.env.clone();
    if let Some(path) = environment.get(&OsString::from("PATH")) {
        let safe = env::join_paths(
            env::split_paths(path).filter(|p| !p.starts_with(&binding.values.project)),
        )
        .map_err(|_| Error::new("invalid teardown tool PATH"))?;
        environment.insert("PATH".into(), safe);
    }
    Ok(Runtime {
        config: binding,
        env: environment,
        binary: tools.openshell.clone(),
        output: crate::output::Output::default(),
    })
}

fn plan(tools: &Tools, config: PathBuf, state: PathBuf, binary: PathBuf) -> Result<Plan> {
    check_tree(
        &config,
        &["projects", "transfer-locks", "container-defaults.json"],
    )?;
    check_tree(
        &state,
        &["local-setup", "setup.lock", "uninstall-incomplete.json"],
    )?;
    if present(&state.join("uninstall-incomplete.json"))? {
        return Err(Error::new(
            "an earlier uninstall did not complete; inspect uninstall-incomplete.json and remaining native resources before manually clearing the marker",
        ));
    }
    check_binary(&binary)?;
    let setup = state.join("local-setup");
    let registered = registrations(tools)?.0;
    if let Some(ref record) = registered {
        compatible_registration(record)?;
    }
    let gateway = exists(tools, "container", CONTAINER)?;
    let network = exists(tools, "network", GATEWAY)?;
    if gateway != registered.is_some() || (!gateway && (network || present(&setup)?)) {
        return Err(Error::new(
            "incomplete local gateway state; inspect and clean up explicitly before uninstall",
        ));
    }
    let registry = config
        .parent()
        .ok_or_else(|| Error::new("configuration parent required"))?
        .join("openshell/gateways/dungeon-local");
    if present(&registry)? {
        check_tree(&registry, &["metadata.json", "mtls", "last_sandbox"])?;
    }
    if gateway || registered.is_some() || network || present(&setup)? {
        let dir = directory(&setup, false, true)?;
        let (bytes, _) = private_read(&dir, "gateway.toml", 8192, false)?;
        existing_mode(
            std::str::from_utf8(&bytes).map_err(|_| Error::new("invalid gateway configuration"))?,
            false,
        )?;
    }
    let gateway_id = if gateway {
        let image = native_json(
            &tools.podman(
                &["image", "inspect", IMAGE, "--format", "{{json .}}"],
                "uninstall gateway image inspection failed",
            )?,
            "invalid gateway image inventory",
        )?;
        let inspect = native_json(
            &tools.podman(
                &["container", "inspect", CONTAINER],
                "uninstall gateway inspection failed",
            )?,
            "invalid gateway container inventory",
        )?;
        let c = inspect
            .as_array()
            .filter(|a| a.len() == 1)
            .map(|a| &a[0])
            .ok_or_else(|| Error::new("ambiguous gateway container inventory"))?;
        let socket = format!("/run/user/{}/podman/podman.sock", unsafe {
            libc::geteuid()
        });
        if !compatible_container(c, &image, &tools.mappings, &setup, &socket) {
            return Err(Error::new(
                "refusing to uninstall an unrecognized gateway container",
            ));
        }
        Some(id(c["Id"].as_str().unwrap_or_default())?)
    } else {
        None
    };

    let mut bindings = Vec::new();
    let mut bound_ids = BTreeSet::new();
    let mut containers = BTreeSet::new();
    if let Some(ref id) = gateway_id {
        containers.insert(id.clone());
    }
    for entry in crate::binding::list_in(&config.join("projects"))? {
        if entry["binding"]["state"] == "invalid" {
            return Err(Error::new(
                "invalid private binding; inspect before uninstall",
            ));
        }
        let path = entry["config"]
            .as_str()
            .ok_or_else(|| Error::new("invalid binding path"))?;
        let binding = crate::config::load_config(Path::new(path))?;
        // Never delete a project or original imported directory embedded in a removal root.
        let protected = std::iter::once(&binding.values.project)
            .chain(binding.values.imports.iter().map(|i| &i.source))
            .chain(
                binding
                    .values
                    .harness
                    .iter()
                    .flat_map(|h| [&h.source, &h.legacy_auth]),
            );
        for source in protected {
            if source.starts_with(&config)
                || source.starts_with(&state)
                || binary.starts_with(source)
            {
                return Err(Error::new(
                    "uninstall scope overlaps a project or original import",
                ));
            }
        }
        if entry["binding"]["state"] == "bound" {
            if binding.values.gateway != GATEWAY || gateway_id.is_none() || registered.is_none() {
                return Err(Error::new(
                    "bound environment is not on the available local Dungeon gateway; clean it up explicitly first",
                ));
            }
            let runtime = runtime(tools, &binding)?;
            let remote = runtime.inspect_for_destroy()?;
            if !matches!(remote["phase"].as_str(), Some("Ready" | "Stopped")) {
                return Err(Error::new(
                    "sandbox is not Ready or Stopped; inspect before uninstall",
                ));
            }
            let sandbox_id = remote["id"]
                .as_str()
                .ok_or_else(|| Error::new("missing bound sandbox id"))?;
            if !bound_ids.insert(sandbox_id.to_owned()) {
                return Err(Error::new("duplicate bound sandbox identity"));
            }
            let name = format!(
                "openshell-{}--{}-{sandbox_id}",
                binding.values.workspace, binding.values.name
            );
            let container = tools.podman(
                &["container", "inspect", &name, "--format", "{{.Id}}"],
                "bound sandbox container missing; inspect before uninstall",
            )?;
            if !containers.insert(id(&container)?) {
                return Err(Error::new("duplicate sandbox container identity"));
            }
            bindings.push(binding);
        }
    }
    if registered.is_some() && gateway_id.is_some() && sandbox_ids(tools)? != bound_ids {
        return Err(Error::new(
            "gateway has unbound or foreign sandboxes; inspect all workspaces and clean them up explicitly",
        ));
    }
    if network_containers(tools)? != containers {
        return Err(Error::new(
            "Dungeon network has unknown or missing containers; no prune attempted",
        ));
    }
    if network
        && tools
            .podman(
                &["network", "inspect", GATEWAY, "--format", "{{.Driver}}"],
                "network inspection failed",
            )?
            .trim()
            != "bridge"
    {
        return Err(Error::new("unrecognized Dungeon network"));
    }
    Ok(Plan {
        config,
        state,
        binary,
        bindings,
        gateway_id,
        registration: registered.is_some(),
        network,
        containers,
        sandboxes: bound_ids,
    })
}

fn mutate(tools: &Tools, binary: &Path, args: &[&str]) -> Result<()> {
    if !query(binary, &tools.env, args, Duration::from_secs(120)).is_some_and(|r| r.code == 0) {
        return Err(Error::new(
            "uninstall operation failed or timed out; partial removal may remain. Binary and remaining private state retained; inspect before retrying",
        ));
    }
    Ok(())
}

fn wait_for_sandbox_removal(tools: &Tools, plan: &Plan, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let expected: BTreeSet<_> = plan.gateway_id.iter().cloned().collect();
    loop {
        let sandboxes = if plan.registration && plan.gateway_id.is_some() {
            sandbox_ids(tools)?
        } else {
            BTreeSet::new()
        };
        let containers = network_containers(tools)?;
        if !sandboxes.is_subset(&plan.sandboxes) || !containers.is_subset(&plan.containers) {
            return Err(Error::new(
                "new sandbox or container appeared during uninstall; installation retained",
            ));
        }
        let mut absent = sandboxes.is_empty() && containers == expected;
        for container in plan.containers.difference(&expected) {
            absent &= !exists(tools, "container", container)?;
        }
        for sandbox in &plan.sandboxes {
            absent &= !exists(
                tools,
                "volume",
                &format!("openshell-sandbox-{sandbox}-workspace"),
            )?;
            for prefix in [
                "openshell-token-",
                "openshell-proxy-auth-",
                "openshell-tls-ca-",
                "openshell-tls-cert-",
                "openshell-tls-key-",
            ] {
                absent &= !exists(tools, "secret", &format!("{prefix}{sandbox}"))?;
            }
        }
        if absent {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(Error::new(
                "sandbox deletion has not converged: gateway records, containers, volumes or secrets remain; installation retained",
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn execute(tools: &Tools, plan: &Plan, cleanup_timeout: Duration) -> Result<()> {
    // Preserve reviewed IDs across failure after Runtime::destroy retires a binding.
    let state = directory(&plan.state, true, true)?;
    private_create(
        &state,
        "uninstall-incomplete.json",
        &serde_json::to_vec(&serde_json::json!({
            "schema":1, "containers":plan.containers, "sandboxes":plan.sandboxes
        }))?,
    )?;
    for binding in &plan.bindings {
        let runtime = runtime(tools, binding)?;
        runtime.down()?;
        runtime.destroy()?;
    }
    // OpenShell deletion is asynchronous: wait for independent native absence, not just CLI success.
    wait_for_sandbox_removal(tools, plan, cleanup_timeout)?;
    for binding in &plan.bindings {
        if present(&binding.state_path())? {
            return Err(Error::new(
                "runtime binding remains after destruction; installation retained",
            ));
        }
    }
    if let Some(ref container) = plan.gateway_id {
        let current = id(&tools.podman(
            &["container", "inspect", CONTAINER, "--format", "{{.Id}}"],
            "gateway identity recheck failed",
        )?)?;
        if &current != container {
            return Err(Error::new(
                "gateway identity changed; installation retained",
            ));
        }
        mutate(tools, &tools.podman, &["rm", "--force", container])?;
        if exists(tools, "container", container)? || exists(tools, "container", CONTAINER)? {
            return Err(Error::new(
                "gateway container remains; installation retained",
            ));
        }
    }
    if !network_containers(tools)?.is_empty() {
        return Err(Error::new("network is not empty; installation retained"));
    }
    if plan.network {
        // No --force: another user's new container must prevent network removal.
        mutate(tools, &tools.podman, &["network", "rm", GATEWAY])?;
        if exists(tools, "network", GATEWAY)? {
            return Err(Error::new("Dungeon network remains; installation retained"));
        }
    }
    if plan.registration {
        mutate(tools, &tools.openshell, &["gateway", "remove", GATEWAY])?;
    }
    if registrations(tools)?.0.is_some() {
        return Err(Error::new(
            "Dungeon gateway registration remains; installation retained",
        ));
    }
    let registry = tools
        .env
        .get(&OsString::from("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(tools.env.get(&OsString::from("HOME")).unwrap()).join(".config")
        })
        .join("openshell/gateways/dungeon-local");
    if present(&registry)? {
        // OpenShell 0.0.116 removes metadata, but deliberately leaves its mTLS directory.
        if present(&registry.join("metadata.json"))? {
            return Err(Error::new(
                "gateway metadata remains; installation retained",
            ));
        }
        check_tree(&registry, &["mtls", "last_sandbox"])?;
        fs::remove_dir_all(&registry)?;
    }
    if present(&registry)? {
        return Err(Error::new(
            "gateway client TLS files remain; installation retained",
        ));
    }
    check_tree(
        &plan.config,
        &["projects", "transfer-locks", "container-defaults.json"],
    )?;
    check_tree(
        &plan.state,
        &["local-setup", "setup.lock", "uninstall-incomplete.json"],
    )?;
    check_binary(&plan.binary)?;
    for path in [&plan.config, &plan.state] {
        if present(path)? {
            fs::remove_dir_all(path)?;
        }
    }
    if present(&plan.binary)? {
        fs::remove_file(&plan.binary)?;
    }
    for path in [&plan.config, &plan.state, &plan.binary] {
        if present(path)? {
            return Err(Error::new("uninstall path remains; removal is incomplete"));
        }
    }
    Ok(())
}

pub(crate) fn run(bin_dir: Option<PathBuf>, dry_run: bool, yes: bool) -> Result<i32> {
    if !dry_run && !yes && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        return Err(Error::new(
            "uninstall requires a terminal or explicit --yes; use --dry-run to inspect",
        ));
    }
    let home = PathBuf::from(env::var_os("HOME").ok_or_else(|| Error::new("HOME required"))?);
    let cwd = env::current_dir()?.canonicalize()?;
    let project = (cwd != home.canonicalize()?).then_some(cwd.as_path());
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("dungeon");
    let state = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("dungeon");
    let binary = bin_dir
        .unwrap_or_else(|| home.join(".local/bin"))
        .join("dungeon");
    for path in [&config, &state, &binary] {
        if !safe_path(
            path.parent().unwrap(),
            &project.into_iter().collect::<Vec<_>>(),
        ) {
            return Err(Error::new(
                "uninstall paths must be normalized, symlink-free and outside the current project",
            ));
        }
    }
    if config.starts_with(&state)
        || state.starts_with(&config)
        || binary.starts_with(&config)
        || binary.starts_with(&state)
    {
        return Err(Error::new("uninstall roots must not overlap"));
    }
    if !present(&config)? && !present(&state)? {
        return binary_only(binary.parent().map(Path::to_path_buf), dry_run, yes);
    }
    let entries = crate::binding::list_in(&config.join("projects"))?;
    let empty_state = if present(&state)? {
        check_tree(&state, &[]).is_ok()
    } else {
        true
    };
    if entries.is_empty()
        && present(&config.join("container-defaults.json"))?
        && empty_state
        && !present(
            &config
                .parent()
                .unwrap()
                .join("openshell/gateways/dungeon-local"),
        )?
    {
        return remove_plain(&[], &config, &state, &binary, dry_run, yes);
    }
    let mut plain = vec![];
    for entry in &entries {
        if entry["binding"]["state"] == "invalid" {
            return Err(Error::new(
                "invalid private binding; inspect before uninstall",
            ));
        }
        let binding = crate::config::load_config(Path::new(
            entry["config"]
                .as_str()
                .ok_or_else(|| Error::new("invalid binding path"))?,
        ))?;
        if binding.values.container.is_some() {
            plain.push(binding);
        }
    }
    if !plain.is_empty() {
        let mixed = plain.len() != entries.len()
            || !empty_state
            || present(
                &config
                    .parent()
                    .unwrap()
                    .join("openshell/gateways/dungeon-local"),
            )?;
        if mixed && plain.iter().any(|c| c.state_path().exists()) {
            return Err(Error::new(
                "mixed runtime installation: destroy plain containers first, then retry uninstall; --binary-only removes only the CLI",
            ));
        }
        if !mixed {
            return remove_plain(&plain, &config, &state, &binary, dry_run, yes);
        }
        crate::container::removal_inventory(&plain)?.verify_empty()?;
    }
    let tools = prerequisites(env::consts::OS, project, setup_environment(project)?)?;
    let plan = plan(&tools, config, state, binary)?;
    println!(
        "{}",
        crate::config::review_json(&serde_json::json!({
            "operation": "uninstall", "dry_run": dry_run,
            "bound_sandboxes": plan.bindings.len(), "containers": plan.containers,
            "gateway": plan.gateway_id.as_ref().map(|_| GATEWAY), "network": plan.network,
            "remove_paths": [&plan.config, &plan.state, &plan.binary],
            "preserve": ["host projects and original imports", "OpenShell and Podman", "cached images", "other gateways and containers"],
            "warning": "Guest files, sessions, authentication and private Dungeon configuration are permanently lost. Close all Dungeon clients first; no concurrent lifecycle operations."
        }))?
    );
    if dry_run {
        return Ok(0);
    }
    if !yes {
        print!("Type `uninstall dungeon` to confirm: ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if answer.trim_end_matches(['\r', '\n']) != "uninstall dungeon" {
            return Err(Error::new("uninstall cancelled; nothing removed"));
        }
    }
    let _lock = setup_lock(&plan.state)?;
    // Rebuild the inventory after confirmation/locking before the first destructive operation.
    let current = self::plan(
        &tools,
        plan.config.clone(),
        plan.state.clone(),
        plan.binary.clone(),
    )?;
    if current.containers != plan.containers
        || current.registration != plan.registration
        || current.network != plan.network
    {
        return Err(Error::new(
            "uninstall inventory changed; review a fresh dry-run",
        ));
    }
    execute(&tools, &current, Duration::from_secs(30))?;
    println!(
        "Dungeon uninstalled; local sandbox/gateway containers, network, registration, private state and selected binary are absent."
    );
    Ok(0)
}

fn approve_removal(yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::new(
            "uninstall requires a terminal or explicit --yes",
        ));
    }
    print!("Type `uninstall dungeon` to confirm: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim() != "uninstall dungeon" {
        return Err(Error::new("uninstall cancelled; nothing removed"));
    }
    Ok(())
}

pub(crate) fn binary_only(bin_dir: Option<PathBuf>, dry_run: bool, yes: bool) -> Result<i32> {
    let home = PathBuf::from(env::var_os("HOME").ok_or_else(|| Error::new("HOME required"))?);
    let binary = bin_dir
        .unwrap_or_else(|| home.join(".local/bin"))
        .join("dungeon");
    check_binary(&binary)?;
    println!(
        "{}",
        crate::config::review_json(
            &serde_json::json!({"operation":"uninstall-binary","dry_run":dry_run,"remove_path":binary,"container_cleanup":"not verified; configuration and runtime resources retained"})
        )?
    );
    if dry_run {
        return Ok(0);
    }
    approve_removal(yes)?;
    check_binary(&binary)?;
    if present(&binary)? {
        fs::remove_file(&binary)?;
    }
    if present(&binary)? {
        return Err(Error::new("binary removal incomplete"));
    }
    println!("CLI removed. Container cleanup not verified.");
    Ok(0)
}

fn remove_plain(
    bindings: &[Config],
    config: &Path,
    state: &Path,
    binary: &Path,
    dry: bool,
    yes: bool,
) -> Result<i32> {
    check_tree(
        config,
        &["projects", "transfer-locks", "container-defaults.json"],
    )?;
    check_binary(binary)?;
    check_tree(state, &[])?;
    for binding in bindings {
        for source in std::iter::once(&binding.values.project)
            .chain(binding.values.imports.iter().map(|i| &i.source))
            .chain(
                binding
                    .values
                    .harness
                    .iter()
                    .flat_map(|h| [&h.source, &h.legacy_auth]),
            )
        {
            if source.starts_with(config) || source.starts_with(state) || binary.starts_with(source)
            {
                return Err(Error::new(
                    "uninstall scope overlaps a project or original import",
                ));
            }
        }
    }
    let inventory = crate::container::removal_inventory(bindings)?;
    let ids = &inventory.ids;
    println!(
        "{}",
        crate::config::review_json(
            &serde_json::json!({"operation":"uninstall","dry_run":dry,"containers":ids,"remove_paths":[config,binary]})
        )?
    );
    if dry {
        return Ok(0);
    }
    approve_removal(yes)?;
    if &crate::container::removal_inventory(bindings)?.ids != ids {
        return Err(Error::new("uninstall inventory changed"));
    }
    for binding in bindings {
        if binding.state_path().try_exists()? {
            let runtime = crate::container::Runtime::new(binding)?;
            runtime.down()?;
            runtime.destroy()?;
        }
    }
    inventory.verify_empty()?;
    check_tree(
        config,
        &["projects", "transfer-locks", "container-defaults.json"],
    )?;
    check_binary(binary)?;
    check_tree(state, &[])?;
    fs::remove_dir_all(config)?;
    if present(state)? {
        fs::remove_dir(state)?;
    }
    if present(binary)? {
        fs::remove_file(binary)?;
    }
    println!("Recorded containers, private configuration and selected CLI removed.");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    struct Fixture {
        root: tempfile::TempDir,
        tools: Tools,
        config: PathBuf,
        state: PathBuf,
        binary: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("config/dungeon");
            let state = root.path().join("state/dungeon");
            let binary = root.path().join("bin/dungeon");
            let script = root.path().join("native");
            directory(&config, true, true).unwrap();
            directory(&state.join("local-setup"), true, true).unwrap();
            directory(binary.parent().unwrap(), true, true).unwrap();
            fs::copy(env::current_exe().unwrap(), &binary).unwrap();
            fs::write(state.join("local-setup/gateway.toml"), LIVE_CONFIG).unwrap();
            fs::set_permissions(
                state.join("local-setup/gateway.toml"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
            let registry = root.path().join("config/openshell/gateways/dungeon-local");
            directory(&registry, true, true).unwrap();
            fs::write(registry.join("metadata.json"), "synthetic").unwrap();
            fs::write(registry.join("last_sandbox"), "synthetic-id").unwrap();
            directory(&registry.join("mtls"), true, true).unwrap();
            fs::write(registry.join("mtls/tls.key"), "synthetic private key").unwrap();
            fs::write(root.path().join("registrations.json"), serde_json::json!([
                {"name":GATEWAY,"active":true,"endpoint":ENDPOINT,"type":"local","auth":"mtls","source":"user"},
                {"name":"other","active":false}
            ]).to_string()).unwrap();
            fs::write(
                root.path().join("image.json"),
                super::super::tests::image().to_string(),
            )
            .unwrap();
            let socket = format!("/run/user/{}/podman/podman.sock", unsafe {
                libc::geteuid()
            });
            fs::write(
                root.path().join("gateway.json"),
                serde_json::json!([super::super::tests::container(
                    &state.join("local-setup"),
                    &socket
                )])
                .to_string(),
            )
            .unwrap();
            fs::write(root.path().join("network"), "").unwrap();
            fs::write(root.path().join("project"), "host project preserved").unwrap();
            fs::write(root.path().join("unrelated-container"), "preserved").unwrap();
            fs::write(
                &script,
                r#"#!/bin/sh
set -eu
case "$1 $2" in
 'image inspect') cat "$FIXTURE/image.json" ;;
 'container exists')
   case "$3" in openshell-gateway-dungeon-local|aaaaaaaa*) test -f "$FIXTURE/gateway.json";; *) test -f "$FIXTURE/sandbox-container";; esac ;;
 'container inspect')
   if test "$#" = 3; then cat "$FIXTURE/gateway.json";
   elif test "$3" = openshell-gateway-dungeon-local; then printf '%064d\n' 0 | tr 0 a;
   else test -f "$FIXTURE/sandbox-container"; printf '%064d\n' 0 | tr 0 d; fi ;;
 'volume exists') test -f "$FIXTURE/residual-volume" ;;
 'secret exists') test -f "$FIXTURE/residual-secret" ;;
 'network exists') test -f "$FIXTURE/network" ;;
 'network inspect') printf 'bridge\n' ;;
 'gateway list') cat "$FIXTURE/registrations.json" ;;
 '--gateway dungeon-local')
   if test "$3" = sandbox; then
     for last do :; done
     if test "$last" = 0; then
        if test -f "$FIXTURE/remote.json"; then printf 'synthetic-id\n'; fi
        if test -f "$FIXTURE/retiring"; then printf 'synthetic-id\n'; rm "$FIXTURE/retiring"; fi
       if test -f "$FIXTURE/foreign-sandbox"; then printf 'foreign-id\n'; fi
     fi
   else
     shift 4
     case "$1 $2" in
       'sandbox get') cat "$FIXTURE/remote.json" ;;
       'sandbox stop')
         printf 'sandbox stop\n' >> "$FIXTURE/mutations"
         sed 's/"phase":"Ready"/"phase":"Stopped"/' "$FIXTURE/remote.json" > "$FIXTURE/next.json"
         mv "$FIXTURE/next.json" "$FIXTURE/remote.json" ;;
       'sandbox delete')
         printf 'sandbox delete\n' >> "$FIXTURE/mutations"
         test ! -f "$FIXTURE/fail-delete" || exit 42
          rm "$FIXTURE/remote.json"
          if test -f "$FIXTURE/delay-list"; then touch "$FIXTURE/retiring"; fi
         test ! -f "$FIXTURE/lie-delete" || exit 0
         rm "$FIXTURE/sandbox-container" ;;
       *) exit 94 ;;
     esac
   fi ;;
 'ps --all')
   if test -f "$FIXTURE/gateway.json"; then printf '%064d\n' 0 | tr 0 a; fi
   if test -f "$FIXTURE/sandbox-container"; then printf '%064d\n' 0 | tr 0 d; fi
   if test -f "$FIXTURE/unknown-container"; then printf '%064d\n' 0 | tr 0 c; fi ;;
 'rm --force')
   printf 'gateway remove\n' >> "$FIXTURE/mutations"
   test ! -f "$FIXTURE/fail-remove" || exit 42
   test ! -f "$FIXTURE/lie-remove" || exit 0
   rm "$FIXTURE/gateway.json" ;;
 'network rm') printf 'network remove\n' >> "$FIXTURE/mutations"; rm "$FIXTURE/network" ;;
 'gateway remove')
   printf 'registration remove\n' >> "$FIXTURE/mutations"
   printf '[{"name":"other","active":false}]' > "$FIXTURE/registrations.json"
   rm "$XDG_CONFIG_HOME/openshell/gateways/dungeon-local/metadata.json" ;;
 *) exit 93 ;;
esac
"#,
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
            let tools = Tools {
                openshell: script.clone(),
                podman: script,
                podman_version: "5.8.4".into(),
                mappings: super::super::tests::host_maps(),
                env: HashMap::from([
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("HOME".into(), root.path().as_os_str().into()),
                    (
                        "XDG_CONFIG_HOME".into(),
                        root.path().join("config").into_os_string(),
                    ),
                    ("FIXTURE".into(), root.path().as_os_str().into()),
                ]),
            };
            Self {
                root,
                tools,
                config,
                state,
                binary,
            }
        }

        fn plan(&self) -> Result<Plan> {
            plan(
                &self.tools,
                self.config.clone(),
                self.state.clone(),
                self.binary.clone(),
            )
        }

        fn bind(&self, phase: &str) -> PathBuf {
            let project = self.root.path().join("host-project");
            fs::create_dir(&project).unwrap();
            fs::write(project.join("keep"), "host project").unwrap();
            let key =
                crate::config::digest(&serde_json::json!({"project":project.to_string_lossy()}))
                    .unwrap();
            let directory = self.config.join("projects").join(key);
            crate::config::create_private_dir(&directory).unwrap();
            let policy = crate::policy::starter_policy();
            crate::config::write_new(&directory.join("policy.json"), &policy).unwrap();
            let config_path = directory.join("dungeon.json");
            crate::config::write_new(
                &config_path,
                &serde_json::json!({
                    "schema":2, "name":"synthetic", "project":project, "gateway":GATEWAY,
                "workspace":"default", "image":"localhost/test:1", "agent":"opencode", "providers":[]
                }),
            )
            .unwrap();
            let config = crate::config::load_config(&config_path).unwrap();
            crate::config::write_new(
                &config.state_path(),
                &serde_json::json!({
                    "schema":1, "id":"synthetic-id", "config":config.fingerprint,
                    "observed_policy":crate::config::digest(&policy).unwrap()
                }),
            )
            .unwrap();
            fs::write(self.root.path().join("remote.json"), serde_json::json!({
                "id":"synthetic-id", "name":"synthetic", "workspace":"default", "phase":phase,
                "labels":{"agentarchaeology.ai/dungeon-config":config.fingerprint}, "policy":policy
            }).to_string()).unwrap();
            fs::write(self.root.path().join("sandbox-container"), "").unwrap();
            config_path
        }
    }

    #[test]
    fn dry_inventory_is_read_only_and_removal_verifies_native_absence_before_files() {
        let f = Fixture::new();
        let plan = f.plan().unwrap();
        assert!(!f.root.path().join("mutations").exists());
        assert!(f.binary.exists() && f.state.exists() && f.config.exists());
        execute(&f.tools, &plan, Duration::from_secs(1)).unwrap();
        for path in [
            &f.binary,
            &f.config,
            &f.state,
            &f.root.path().join("gateway.json"),
            &f.root.path().join("network"),
        ] {
            assert!(!path.exists(), "{} remains", path.display());
        }
        assert!(f.root.path().join("project").exists());
        assert!(f.root.path().join("unrelated-container").exists());
        assert_eq!(registrations(&f.tools).unwrap().0, None);
        assert_eq!(
            fs::read_to_string(f.root.path().join("mutations")).unwrap(),
            "gateway remove\nnetwork remove\nregistration remove\n"
        );
        let repeat = f.plan().unwrap();
        execute(&f.tools, &repeat, Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn failed_or_falsely_successful_native_removal_retains_installation() {
        for fault in ["fail-remove", "lie-remove"] {
            let f = Fixture::new();
            let plan = f.plan().unwrap();
            fs::write(f.root.path().join(fault), "").unwrap();
            assert!(execute(&f.tools, &plan, Duration::from_secs(1)).is_err());
            assert!(f.binary.exists() && f.state.exists() && f.config.exists());
            assert!(f.root.path().join("network").exists());
            assert!(registrations(&f.tools).unwrap().0.is_some());
        }
    }

    #[test]
    fn unknown_resources_and_paths_refuse_before_any_deletion() {
        for fault in [
            "foreign-sandbox",
            "unknown-container",
            "unknown-state",
            "binary-link",
            "different-binary",
            "config-link",
            "invalid-binding",
            "unknown-registration",
        ] {
            let f = Fixture::new();
            match fault {
                "unknown-state" => fs::write(f.state.join("foreign"), "preserve").unwrap(),
                "binary-link" => {
                    fs::remove_file(&f.binary).unwrap();
                    symlink(f.root.path().join("project"), &f.binary).unwrap();
                }
                "different-binary" => fs::write(&f.binary, "different install").unwrap(),
                "config-link" => {
                    symlink(f.root.path().join("project"), f.config.join("projects")).unwrap()
                }
                "invalid-binding" => fs::create_dir_all(f.config.join("projects/broken")).unwrap(),
                "unknown-registration" => fs::write(
                    f.config
                        .parent()
                        .unwrap()
                        .join("openshell/gateways/dungeon-local/foreign"),
                    "preserve",
                )
                .unwrap(),
                _ => fs::write(f.root.path().join(fault), "").unwrap(),
            }
            assert!(f.plan().is_err(), "accepted {fault}");
            assert!(!f.root.path().join("mutations").exists());
            assert!(f.root.path().join("project").exists());
        }
    }

    #[test]
    fn changed_gateway_identity_is_not_removed() {
        let f = Fixture::new();
        let mut plan = f.plan().unwrap();
        plan.gateway_id = Some("b".repeat(64));
        plan.containers = ["b".repeat(64)].into_iter().collect();
        assert!(execute(&f.tools, &plan, Duration::from_secs(1)).is_err());
        assert!(!f.root.path().join("mutations").exists());
        assert!(f.binary.exists());
    }

    #[test]
    fn ready_and_stopped_bindings_use_existing_identity_checked_destruction() {
        for phase in ["Ready", "Stopped"] {
            let f = Fixture::new();
            f.bind(phase);
            let plan = f.plan().unwrap();
            assert_eq!(plan.bindings.len(), 1);
            assert_eq!(plan.containers.len(), 2);
            execute(&f.tools, &plan, Duration::from_secs(1)).unwrap();
            assert!(!f.binary.exists() && !f.config.exists() && !f.state.exists());
            assert!(!f.root.path().join("sandbox-container").exists());
            assert_eq!(
                fs::read_to_string(f.root.path().join("host-project/keep")).unwrap(),
                "host project"
            );
            let calls = fs::read_to_string(f.root.path().join("mutations")).unwrap();
            assert_eq!(calls.contains("sandbox stop"), phase == "Ready");
            assert!(
                calls.contains(
                    "sandbox delete\ngateway remove\nnetwork remove\nregistration remove\n"
                )
            );
        }
    }

    #[test]
    fn sandbox_cleanup_failure_or_residual_resources_never_uninstall_the_binary() {
        for fault in [
            "fail-delete",
            "lie-delete",
            "residual-volume",
            "residual-secret",
        ] {
            let f = Fixture::new();
            f.bind("Stopped");
            let plan = f.plan().unwrap();
            fs::write(f.root.path().join(fault), "").unwrap();
            assert!(
                execute(&f.tools, &plan, Duration::from_secs(1)).is_err(),
                "accepted {fault}"
            );
            assert!(f.binary.exists() && f.config.exists() && f.state.exists());
            assert!(f.root.path().join("gateway.json").exists());
            assert!(f.state.join("uninstall-incomplete.json").exists());
            assert!(
                f.plan().is_err(),
                "partial deletion must not forget residual IDs on retry"
            );
        }
    }

    #[test]
    fn drifted_sandbox_identity_refuses_before_stopping_anything() {
        let f = Fixture::new();
        f.bind("Ready");
        let remote = f.root.path().join("remote.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&remote).unwrap()).unwrap();
        value["id"] = "replaced-id".into();
        fs::write(remote, value.to_string()).unwrap();
        assert!(f.plan().is_err());
        assert!(!f.root.path().join("mutations").exists());
        assert!(f.binary.exists());
    }

    #[test]
    fn asynchronous_gateway_deletion_converges_before_gateway_teardown() {
        let f = Fixture::new();
        f.bind("Stopped");
        fs::write(f.root.path().join("delay-list"), "").unwrap();
        let plan = f.plan().unwrap();
        execute(&f.tools, &plan, Duration::from_secs(2)).unwrap();
        assert!(!f.root.path().join("retiring").exists());
        assert!(!f.binary.exists() && !f.state.exists() && !f.config.exists());
    }
}
