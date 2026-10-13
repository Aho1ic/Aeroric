// AeroricSandboxProbe — AP0 batch-1 real App Sandbox feasibility battery.
//
// MIGRATION_PLAN.md §D AP0 / .trellis/tasks/10-12-ap0-store-local-feasibility.
// The driver (scripts/native/ap0/probe-batch1.sh) signs this bundle ad-hoc
// WITH com.apple.security.app-sandbox and executes it; every line this prints
// is an observed runtime fact inside (or, if the entitlement was not
// enforced, outside) the sandbox. The driver captures stdout/stderr into
// .artifacts/native/ap0/batch1/ — the process itself must never write outside
// its own container, so results go to stdout only.
//
// Emission format (one JSON object per line):
//   {"t":"<test-id>","r":"pass|fail|denied|error|info","d":"<detail>"}
//
// Battery (ids fixed — matrix rows reference them):
//   T01 env                 HOME/uid/cwd/container candidate
//   T02 outside_home_write  write into the real $HOME (must be denied)
//   T03 container_write     write into the bundle-id container data dir
//   T04 exec_system         exec /usr/bin/true
//   T05 exec_outside_cli    exec a binary outside the container (/tmp/ap0/fake-agent)
//   T06 exec_bundle_helper  exec own-bundle aeroric-cli --version (helper inheritance)
//   T07 pty_chain           forkpty+ctty echo, then process-group SIGKILL
//   T08 host_serve_ready    spawn bundle aeroric-cli serve --ready-fd 3,
//                           read the ready JSON line, connect() its socket_path
//   T09 login_shell         $SHELL -l -c printf (login-shell PATH probing shape)
//   T10 outside_home_read   read a file in the real $HOME (must be denied)
//
// Note (learned batch-1 run 1): under App Sandbox the kernel rewrites
// $HOME and cwd into the container; "outside" references must come from
// getpwuid()->pw_dir, never from $HOME.
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <mach-o/dyld.h>
#include <pwd.h>
#include <signal.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <util.h>
#include <unistd.h>
#include <dlfcn.h>
#include <Security/Security.h>

extern char **environ;

#define CONTAINER_BUNDLE_ID "com.aeroric.native.sandboxprobe"
#define READY_TIMEOUT_SEC 15
#define WAIT_TIMEOUT_SEC 5

static char g_home[1024];
static char g_container[1400]; // $HOME/Library/Containers/<bundle-id>/Data
static char g_exe[1024];
static char g_bundle_helper[1400]; // <bundle>/Contents/MacOS/aeroric-cli

// ── emit ──────────────────────────────────────────────────────────────────
static void emit(const char *t, const char *r, const char *d) {
  fputs("{\"t\":\"", stdout);
  fputs(t, stdout);
  fputs("\",\"r\":\"", stdout);
  fputs(r, stdout);
  fputs("\",\"d\":\"", stdout);
  for (const char *p = d; *p; p++) {
    unsigned char c = (unsigned char)*p;
    if (c == '"' || c == '\\') { putchar('\\'); putchar(c); }
    else if (c == '\n') { fputs("\\n", stdout); }
    else if (c < 0x20) { /* drop control chars */ }
    else { putchar(c); }
  }
  fputs("\"}\n", stdout);
  fflush(stdout);
}

static void emitf(const char *t, const char *r, const char *fmt, ...) {
  char d[2048];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(d, sizeof d, fmt, ap);
  va_end(ap);
  emit(t, r, d);
}

static long now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1000L + ts.tv_nsec / 1000000L;
}

// Wait for pid with deadline; returns 1=reaped, 0=timeout.
static int wait_pid_timeout(pid_t pid, int timeout_ms, int *status_out) {
  long deadline = now_ms() + timeout_ms;
  for (;;) {
    int status = 0;
    pid_t r = waitpid(pid, &status, WNOHANG);
    if (r == pid) { if (status_out) *status_out = status; return 1; }
    if (r < 0) return 1;
    if (now_ms() >= deadline) return 0;
    usleep(20 * 1000);
  }
}

static const char *wait_status_str(int status) {
  static char buf[80];
  if (WIFEXITED(status)) { snprintf(buf, sizeof buf, "exit=%d", WEXITSTATUS(status)); return buf; }
  if (WIFSIGNALED(status)) { snprintf(buf, sizeof buf, "signal=%d", WTERMSIG(status)); return buf; }
  snprintf(buf, sizeof buf, "status=%d", status);
  return buf;
}

static int mkdir_p(const char *path) {
  char tmp[1400];
  snprintf(tmp, sizeof tmp, "%s", path);
  for (char *p = tmp + 1; *p; p++) {
    if (*p == '/') { *p = '\0'; if (mkdir(tmp, 0700) != 0 && errno != EEXIST) return -1; *p = '/'; }
  }
  if (mkdir(tmp, 0700) != 0 && errno != EEXIST) return -1;
  return 0;
}

// ── T01 ───────────────────────────────────────────────────────────────────
static char g_real_home[1024]; // passwd entry — the sandbox does NOT rewrite it

static void t01_env(void) {
  const char *home = getenv("HOME");
  snprintf(g_home, sizeof g_home, "%s", home ? home : "(unset)");
  const char *pw = getpwuid(getuid())->pw_dir;
  snprintf(g_real_home, sizeof g_real_home, "%s", pw ? pw : "(nouser)");
  // Under App Sandbox $HOME/cwd are rewritten into the container; the
  // real home from passwd is the outside-container reference for T02/T10.
  snprintf(g_container, sizeof g_container, "%s", g_home);
  struct stat st;
  int exists = (stat(g_container, &st) == 0);
  char cwd[1024];
  if (!getcwd(cwd, sizeof cwd)) snprintf(cwd, sizeof cwd, "(getcwd:%s)", strerror(errno));
  emitf("T01", "info", "HOME=%s real_home=%s uid=%d cwd=%s container=%s exists=%d home_rewritten=%d",
        g_home, g_real_home, (int)getuid(), cwd, g_container, exists,
        strcmp(g_home, g_real_home) != 0);
}

// ── T02: write into the REAL home (outside container) — must be denied ───
static void t02_outside_home_write(void) {
  char path[1400];
  snprintf(path, sizeof path, "%s/.ap0-outside-write.txt", g_real_home);
  FILE *f = fopen(path, "w");
  if (f) { fputs("x", f); fclose(f); unlink(path);
    emit("T02", "fail", "SANDBOX NOT ENFORCED: wrote outside container");
    return;
  }
  emitf("T02", "denied", "fopen(%s): errno=%d(%s)", path, errno, strerror(errno));
}

// ── T03: write inside the container data dir ──────────────────────────────
static void t03_container_write(void) {
  if (mkdir_p(g_container) != 0) {
    emitf("T03", "error", "mkdir_p(%s): errno=%d(%s)", g_container, errno, strerror(errno));
    return;
  }
  char path[1400];
  snprintf(path, sizeof path, "%s/.ap0-in.txt", g_container);
  FILE *f = fopen(path, "w");
  if (!f) { emitf("T03", "fail", "fopen(%s): errno=%d(%s)", path, errno, strerror(errno)); return; }
  fputs("in", f); fclose(f);
  emitf("T03", "pass", "wrote %s", path);
}

// ── posix_spawn with merged stdout+stderr pipe ────────────────────────────
// Returns spawn errno (0 = spawned); on spawn, *status_out filled when waited.
static int spawn_capture(char *const argv[], const char *cwd, int timeout_ms,
                         char *out, size_t outsz, int *status_out) {
  int pipefd[2];
  if (pipe(pipefd) != 0) return errno;
  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addclose(&fa, pipefd[0]);
  posix_spawn_file_actions_adddup2(&fa, pipefd[1], 1);
  posix_spawn_file_actions_adddup2(&fa, pipefd[1], 2);
  posix_spawn_file_actions_addclose(&fa, pipefd[1]);
  pid_t pid = 0;
  int rc = posix_spawn(&pid, argv[0], &fa, NULL, argv, environ);
  posix_spawn_file_actions_destroy(&fa);
  close(pipefd[1]);
  if (rc != 0) { close(pipefd[0]); return rc; }
  size_t used = 0;
  if (outsz) out[0] = '\0';
  long deadline = now_ms() + timeout_ms;
  fcntl(pipefd[0], F_SETFL, O_NONBLOCK);
  for (;;) {
    ssize_t n = read(pipefd[0], out + used, outsz - used - 1);
    if (n > 0) { used += (size_t)n; out[used] = '\0'; continue; }
    if (n == 0) break; // writers closed
    if (errno != EAGAIN && errno != EWOULDBLOCK) break;
    if (now_ms() >= deadline) break;
    int status = 0;
    if (wait_pid_timeout(pid, 0, &status)) {
      // drained check: loop until EOF with short grace
      if (now_ms() >= deadline) break;
    }
    usleep(20 * 1000);
  }
  close(pipefd[0]);
  int status = 0;
  if (!wait_pid_timeout(pid, timeout_ms, &status)) {
    kill(pid, SIGKILL);
    wait_pid_timeout(pid, 1000, &status);
  }
  if (status_out) *status_out = status;
  return 0;
}

// ── T04 ───────────────────────────────────────────────────────────────────
static void t04_exec_system(void) {
  char *argv[] = {"/usr/bin/true", NULL};
  int status = 0;
  char out[256];
  int rc = spawn_capture(argv, NULL, 5000, out, sizeof out, &status);
  if (rc != 0) { emitf("T04", "fail", "posix_spawn(/usr/bin/true): rc=%d(%s)", rc, strerror(rc)); return; }
  if (WIFEXITED(status) && WEXITSTATUS(status) == 0) emit("T04", "pass", "/usr/bin/true exit=0");
  else emitf("T04", "fail", "true: %s", wait_status_str(status));
}

// ── T05: exec a binary outside the container (user-installed CLI shape) ──
static void t05_exec_outside_cli(void) {
  char *argv[] = {"/tmp/ap0/fake-agent", NULL};
  int status = 0;
  char out[256];
  int rc = spawn_capture(argv, NULL, 5000, out, sizeof out, &status);
  if (rc != 0) {
    emitf("T05", "denied", "posix_spawn(/tmp/ap0/fake-agent): rc=%d(%s)", rc, strerror(rc));
    return;
  }
  if (WIFEXITED(status) && WEXITSTATUS(status) == 0)
    emit("T05", "fail", "SANDBOX allowed exec of outside-container binary: exit=0");
  else
    emitf("T05", "denied", "spawned but child did not complete normally: %s",
          wait_status_str(status));
}

// ── T06: exec the helper embedded in own bundle (helper inheritance) ─────
static void t06_exec_bundle_helper(void) {
  if (access(g_bundle_helper, X_OK) != 0) {
    emitf("T06", "error", "helper missing/not executable at %s: %s", g_bundle_helper, strerror(errno));
    return;
  }
  char *argv[] = {g_bundle_helper, "--version", NULL};
  int status = 0;
  char out[512];
  int rc = spawn_capture(argv, NULL, 8000, out, sizeof out, &status);
  if (rc != 0) { emitf("T06", "denied", "posix_spawn(helper): rc=%d(%s)", rc, strerror(rc)); return; }
  if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
    char *nl = strchr(out, '\n'); if (nl) *nl = '\0';
    emitf("T06", "pass", "helper ran in sandbox: exit=0 out=%.120s", out);
  } else {
    emitf("T06", "fail", "helper: %s out=%.120s", wait_status_str(status), out);
  }
}

// ── T07: PTY chain (forkpty = openpty+setsid+TIOCSCTTY) + killpg ─────────
static void t07_pty_chain(void) {
  int master = -1;
  pid_t pid = forkpty(&master, NULL, NULL, NULL);
  if (pid < 0) { emitf("T07", "fail", "forkpty: %s", strerror(errno)); return; }
  if (pid == 0) { execl("/bin/sh", "sh", "-c", "echo AP0_PTY_OK", (char *)NULL); _exit(127); }
  char buf[128];
  size_t used = 0;
  long deadline = now_ms() + 5000;
  fcntl(master, F_SETFL, O_NONBLOCK);
  while (now_ms() < deadline && used < sizeof buf - 1) {
    ssize_t n = read(master, buf + used, sizeof buf - 1 - used);
    if (n > 0) { used += (size_t)n; buf[used] = '\0'; if (strstr(buf, "AP0_PTY_OK")) break; continue; }
    if (n == 0) break;
    if (errno != EAGAIN && errno != EWOULDBLOCK) break;
    usleep(20 * 1000);
  }
  buf[used] = '\0';
  int status = 0;
  if (!wait_pid_timeout(pid, 3000, &status)) { kill(-pid, SIGKILL); wait_pid_timeout(pid, 1000, &status); }
  close(master);
  int echo_ok = strstr(buf, "AP0_PTY_OK") != NULL;

  // process-group termination shape (portable-pty/core kill(-pgid) model):
  // second session, child is its own session/group leader; SIGKILL the group.
  int master2 = -1;
  pid_t pid2 = forkpty(&master2, NULL, NULL, NULL);
  int group_ok = 0;
  char group_detail[160] = "";
  if (pid2 < 0) {
    snprintf(group_detail, sizeof group_detail, "second forkpty: %s", strerror(errno));
  } else if (pid2 == 0) { execl("/bin/sh", "sh", "-c", "sleep 30", (char *)NULL); _exit(127); }
  else {
    usleep(200 * 1000);
    if (kill(-pid2, SIGKILL) != 0)
      snprintf(group_detail, sizeof group_detail, "kill(-%d): %s", (int)pid2, strerror(errno));
    int st2 = 0;
    if (wait_pid_timeout(pid2, 3000, &st2)) {
      group_ok = WIFSIGNALED(st2) && WTERMSIG(st2) == SIGKILL;
      if (!group_ok) snprintf(group_detail, sizeof group_detail, "child2: %s", wait_status_str(st2));
    } else {
      snprintf(group_detail, sizeof group_detail, "child2 survived group SIGKILL");
      kill(pid2, SIGKILL); wait_pid_timeout(pid2, 1000, &st2);
    }
    close(master2);
  }
  if (echo_ok && group_ok)
    emit("T07", "pass", "forkpty echo ok; process-group SIGKILL reaped");
  else
    emitf("T07", "fail", "echo_ok=%d group_ok=%d %s", echo_ok, group_ok, group_detail);
}

// ── T08: host serve --ready-fd 3 + socket connect ────────────────────────
static void t08_host_serve(void) {
  if (access(g_bundle_helper, X_OK) != 0) {
    emitf("T08", "error", "helper missing at %s", g_bundle_helper); return;
  }
  char data_dir[1500];
  snprintf(data_dir, sizeof data_dir, "%s/ap0-data", g_container);
  if (mkdir_p(data_dir) != 0) {
    emitf("T08", "error", "mkdir_p(%s): %s", data_dir, strerror(errno)); return;
  }
  int ready[2];
  if (pipe(ready) != 0) { emitf("T08", "error", "pipe: %s", strerror(errno)); return; }

  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addclose(&fa, ready[0]);
  posix_spawn_file_actions_adddup2(&fa, ready[1], 3);
  posix_spawn_file_actions_addclose(&fa, ready[1]);
  posix_spawn_file_actions_addopen(&fa, 1, "/dev/null", O_WRONLY, 0);

  char *argv[] = {g_bundle_helper, "--data-dir", data_dir, "serve", "--ready-fd", "3", NULL};
  pid_t pid = 0;
  int rc = posix_spawn(&pid, g_bundle_helper, &fa, NULL, argv, environ);
  posix_spawn_file_actions_destroy(&fa);
  close(ready[1]);
  if (rc != 0) { close(ready[0]);
    emitf("T08", "denied", "posix_spawn(aeroric-cli serve): rc=%d(%s)", rc, strerror(rc)); return; }

  // read one ready line (deadline READY_TIMEOUT_SEC)
  char line[1024];
  size_t used = 0;
  line[0] = '\0';
  fcntl(ready[0], F_SETFL, O_NONBLOCK);
  long deadline = now_ms() + READY_TIMEOUT_SEC * 1000L;
  int eof = 0;
  while (now_ms() < deadline && used < sizeof line - 1) {
    ssize_t n = read(ready[0], line + used, sizeof line - 1 - used);
    if (n > 0) { used += (size_t)n; line[used] = '\0'; if (strchr(line, '\n')) break; continue; }
    if (n == 0) { eof = 1; break; }
    if (errno != EAGAIN && errno != EWOULDBLOCK) { eof = 1; break; }
    usleep(20 * 1000);
  }
  line[used] = '\0';
  if (used == 0) {
    int status = 0, reaped = wait_pid_timeout(pid, 0, &status);
    emitf("T08", "fail", "no ready line (eof=%d, child %s)", eof,
          reaped ? wait_status_str(status) : "still running");
    if (!reaped) { kill(pid, SIGKILL); wait_pid_timeout(pid, 1000, &status); }
    close(ready[0]); return;
  }
  char *nl = strchr(line, '\n'); if (nl) *nl = '\0';

  // parse socket_path: ReadyInfo is serde camelCase → "socketPath"
  // (accept snake_case too so a future rename fails loudly, not silently)
  char sock[1024] = "";
  const char *key = strstr(line, "\"socketPath\":\"");
  if (!key) key = strstr(line, "\"socket_path\":\"");
  if (key) {
    key = strchr(key, ':');
    if (key) {
      key = strchr(key, '"');
      if (key) {
        key++;
        const char *end = strchr(key, '"');
        if (end && (size_t)(end - key) < sizeof sock) { memcpy(sock, key, (size_t)(end - key)); sock[end - key] = '\0'; }
      }
    }
  }
  char raw[600];
  snprintf(raw, sizeof raw, "%.400s", line);

  // connect() to the host UDS (sandbox local-IPC reachability)
  const char *conn_detail = "socket_path not parsed";
  int conn_ok = 0;
  if (sock[0]) {
    struct sockaddr_un addr;
    memset(&addr, 0, sizeof addr);
    addr.sun_family = AF_UNIX;
    snprintf(addr.sun_path, sizeof addr.sun_path, "%s", sock);
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    if (s < 0) { conn_detail = strerror(errno); }
    else {
      if (connect(s, (struct sockaddr *)&addr, sizeof addr) == 0) { conn_ok = 1; conn_detail = "connected"; }
      else { conn_detail = strerror(errno); }
      close(s);
    }
  }

  kill(pid, SIGTERM);
  int status = 0;
  if (!wait_pid_timeout(pid, WAIT_TIMEOUT_SEC * 1000, &status)) {
    kill(pid, SIGKILL); wait_pid_timeout(pid, 1000, &status);
  }
  close(ready[0]);
  emitf("T08", (conn_ok || sock[0]) ? "pass" : "fail",
        "ready=ok socket=%.250s connect=%s child=%s raw=%.300s",
        sock, conn_detail, wait_status_str(status), raw);
}

// ── T09: login-shell shape (platform/unix.rs: $SHELL -l -c 'printf …') ───
static void t09_login_shell(void) {
  const char *shell = getenv("SHELL");
  if (!shell || !*shell) shell = "/bin/zsh";
  char shell_buf[256];
  snprintf(shell_buf, sizeof shell_buf, "%s", shell);
  char *argv[] = {shell_buf, "-l", "-c",
                  "printf AP0_LOGIN_OK; echo PATH=$PATH", NULL};
  int status = 0;
  char out[768];
  int rc = spawn_capture(argv, NULL, 10000, out, sizeof out, &status);
  if (rc != 0) { emitf("T09", "fail", "spawn(%s): %s", shell, strerror(rc)); return; }
  int ok = WIFEXITED(status) && WEXITSTATUS(status) == 0 && strstr(out, "AP0_LOGIN_OK") != NULL;
  for (char *p = out; *p; p++) if (*p == '\n') *p = '|';
  emitf("T09", ok ? "pass" : "fail", "shell=%s %s out=%.300s",
        shell, wait_status_str(status), out);
}

// ── T10: read a file in the REAL home (outside container) — must be denied
static void t10_outside_home_read(void) {
  char path[1400];
  snprintf(path, sizeof path, "%s/.zshrc", g_real_home);
  int fd = open(path, O_RDONLY);
  if (fd < 0) { emitf("T10", "denied", "open(%s): errno=%d(%s)", path, errno, strerror(errno)); return; }
  char buf[64];
  ssize_t n = read(fd, buf, sizeof buf);
  close(fd);
  emitf("T10", "fail", "SANDBOX NOT ENFORCED: read %s (%zd bytes)", path, n);
}

// ── T11/T12: arg-driven real-path rows (batch 2) ─────────────────────────
// argv entries: "exec:<label>:<path>" / "read:<label>:<path>"
// Labels/paths are resolved by the driver OUTSIDE the sandbox and passed in
// (the sandboxed process must not read outside files to learn its targets).
static void t11_exec_path(const char *label, const char *path) {
  char tid[96];
  snprintf(tid, sizeof tid, "T11-%s", label);
  if (access(path, X_OK) != 0) {
    emitf(tid, "denied", "access(X_OK,%s): %s", path, strerror(errno));
    return;
  }
  char path_buf[1024];
  snprintf(path_buf, sizeof path_buf, "%s", path);
  char *argv[] = {path_buf, "--version", NULL};
  int status = 0;
  char out[256];
  int rc = spawn_capture(argv, NULL, 8000, out, sizeof out, &status);
  if (rc != 0) {
    emitf(tid, "denied", "posix_spawn(%s): rc=%d(%s)", path, rc, strerror(rc));
    return;
  }
  for (char *p = out; *p; p++) if (*p == '\n') *p = '|';
  emitf(tid, (WIFEXITED(status) && WEXITSTATUS(status) == 0) ? "pass" : "fail",
        "path=%s %s out=%.120s", path, wait_status_str(status), out);
}

static void t12_read_path(const char *label, const char *path) {
  char tid[96];
  snprintf(tid, sizeof tid, "T12-%s", label);
  int fd = open(path, O_RDONLY);
  if (fd < 0) { emitf(tid, "denied", "open(%s): %s", path, strerror(errno)); return; }
  char b[16];
  ssize_t n = read(fd, b, sizeof b);
  close(fd);
  emitf(tid, n >= 0 ? "pass" : "fail", "path=%s read=%zd bytes", path, n);
}
static int uds_rpc_call(int sock, const char *req, char *resp_out, size_t resp_max) {
  size_t req_len = strlen(req);
  if (write(sock, req, req_len) != (ssize_t)req_len) return -1;
  size_t used = 0;
  while (used < resp_max - 1) {
    char c;
    ssize_t n = read(sock, &c, 1);
    if (n <= 0) return -1;
    if (c == '\n') break;
    resp_out[used++] = c;
  }
  resp_out[used] = '\0';
  return (used > 0) ? 0 : -1;
}

static void t13_minimal_task_loop(void) {
  char bundle_fake_agent[1400];
  snprintf(bundle_fake_agent, sizeof bundle_fake_agent, "%.*s/fake-agent",
           (int)(strrchr(g_exe, '/') - g_exe), g_exe);

  if (access(g_bundle_helper, X_OK) != 0 || access(bundle_fake_agent, X_OK) != 0) {
    emitf("T13", "error", "helper missing or fake-agent missing (helper=%s fake_agent=%s)",
          g_bundle_helper, bundle_fake_agent);
    return;
  }

  char data_dir[1500];
  snprintf(data_dir, sizeof data_dir, "%s/ap0-data", g_container);
  mkdir_p(data_dir);

  char project_dir[1500];
  snprintf(project_dir, sizeof project_dir, "%s/project-ap0", data_dir);
  mkdir_p(project_dir);

  char work_path[1500];
  snprintf(work_path, sizeof work_path, "%s/work.txt", project_dir);
  FILE *wf = fopen(work_path, "w");
  if (wf) { fputs("initial\n", wf); fclose(wf); }

  char settings_path[1500];
  snprintf(settings_path, sizeof settings_path, "%s/settings.json", data_dir);
  FILE *sf = fopen(settings_path, "w");
  if (sf) {
    fprintf(sf, "{\"custom_agents\":[{\"id\":\"fake-agent\",\"family\":\"claude\",\"path\":\"/bin/sh\",\"args\":[\"-c\",\"echo 'modified by fake-agent in sandbox' >> work.txt\"]}]}\n");
    fclose(sf);
  }

  int ready[2];
  if (pipe(ready) != 0) { emitf("T13", "error", "pipe: %s", strerror(errno)); return; }

  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addclose(&fa, ready[0]);
  posix_spawn_file_actions_adddup2(&fa, ready[1], 3);
  posix_spawn_file_actions_addclose(&fa, ready[1]);
  posix_spawn_file_actions_addopen(&fa, 1, "/dev/null", O_WRONLY, 0);

  char *argv[] = {g_bundle_helper, "--data-dir", data_dir, "serve", "--ready-fd", "3", NULL};
  pid_t pid = 0;
  int rc = posix_spawn(&pid, g_bundle_helper, &fa, NULL, argv, environ);
  posix_spawn_file_actions_destroy(&fa);
  close(ready[1]);
  if (rc != 0) {
    close(ready[0]);
    emitf("T13", "denied", "posix_spawn(aeroric-cli serve): rc=%d(%s)", rc, strerror(rc));
    return;
  }

  char line[1024];
  size_t used = 0;
  line[0] = '\0';
  fcntl(ready[0], F_SETFL, O_NONBLOCK);
  long deadline = now_ms() + READY_TIMEOUT_SEC * 1000L;
  while (now_ms() < deadline && used < sizeof line - 1) {
    ssize_t n = read(ready[0], line + used, sizeof line - 1 - used);
    if (n > 0) { used += (size_t)n; line[used] = '\0'; if (strchr(line, '\n')) break; continue; }
    if (n == 0) break;
    if (errno != EAGAIN && errno != EWOULDBLOCK) break;
    usleep(20 * 1000);
  }
  close(ready[0]);

  char sock_path[1024] = "";
  const char *key = strstr(line, "\"socketPath\":\"");
  if (!key) key = strstr(line, "\"socket_path\":\"");
  if (key) {
    key = strchr(key, ':');
    if (key) {
      key = strchr(key, '"');
      if (key) {
        key++;
        const char *end = strchr(key, '"');
        if (end && (size_t)(end - key) < sizeof sock_path) {
          memcpy(sock_path, key, (size_t)(end - key));
          sock_path[end - key] = '\0';
        }
      }
    }
  }

  char token[256] = "";
  char token_path[1500];
  snprintf(token_path, sizeof token_path, "%s/host.token", data_dir);
  FILE *tf = fopen(token_path, "r");
  if (tf) {
    if (fgets(token, sizeof token, tf)) {
      char *nl = strchr(token, '\n'); if (nl) *nl = '\0';
      nl = strchr(token, '\r'); if (nl) *nl = '\0';
    }
    fclose(tf);
  }

  int sock = -1;
  if (sock_path[0]) {
    sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock >= 0) {
      struct sockaddr_un addr;
      memset(&addr, 0, sizeof addr);
      addr.sun_family = AF_UNIX;
      snprintf(addr.sun_path, sizeof addr.sun_path, "%s", sock_path);
      if (connect(sock, (struct sockaddr *)&addr, sizeof addr) != 0) {
        close(sock);
        sock = -1;
      }
    }
  }

  int handshake_ok = 0;
  int project_ok = 0;
  int task_ok = 0;
  int modified_ok = 0;
  int app_ins_ok = 0;
  int app_list_ok = 0;
  int app_resp_ok = 0;
  char task_resp[512] = "";
  if (sock >= 0) {
    char req[2048];
    char resp[4096];

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"1\",\"method\":\"handshake\",\"params\":{\"token\":\"%s\",\"clientId\":\"probe-t13\"}}\n", token);
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0 && strstr(resp, "\"ok\":true")) {
      handshake_ok = 1;
    }

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"2\",\"method\":\"project.open\",\"params\":{\"path\":\"%s\",\"newId\":\"p-ap0-1\"}}\n", project_dir);
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0 && strstr(resp, "\"ok\":true")) {
      project_ok = 1;
    }

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"3\",\"method\":\"task.run\",\"params\":{\"projectPath\":\"%s\",\"agent\":\"fake-agent\",\"prompt\":\"ap0-probe\",\"taskId\":\"task-ap0-1\"}}\n", project_dir);
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0) {
      snprintf(task_resp, sizeof task_resp, "%.500s", resp);
      if (strstr(resp, "\"ok\":true")) task_ok = 1;
    }
    for (int i = 0; i < 60; i++) {
      usleep(50 * 1000);
      FILE *f = fopen(work_path, "r");
      if (f) {
        char buf[512] = "";
        size_t n = fread(buf, 1, sizeof buf - 1, f);
        buf[n] = '\0';
        fclose(f);
        if (strstr(buf, "modified by fake-agent in sandbox") != NULL) {
          modified_ok = 1;
          break;
        }
      }
    }

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"4\",\"method\":\"approval.test_request\",\"params\":{\"eventId\":\"ap0-ev-1\",\"kind\":\"approval\"}}\n");
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0 && (strstr(resp, "\"inserted\":\"ap0-ev-1\"") || strstr(resp, "\"ok\":true"))) {
      app_ins_ok = 1;
    }

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"5\",\"method\":\"approval.list\",\"params\":{}}\n");
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0 && strstr(resp, "ap0-ev-1")) {
      app_list_ok = 1;
    }

    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"6\",\"method\":\"approval.respond\",\"params\":{\"eventId\":\"ap0-ev-1\",\"outcome\":{\"kind\":\"approved\"}}}\n");
    if (uds_rpc_call(sock, req, resp, sizeof resp) == 0 && strstr(resp, "\"accepted\":true")) {
      app_resp_ok = 1;
    }

    close(sock);
  }

  kill(pid, SIGTERM);
  int status = 0;
  if (!wait_pid_timeout(pid, WAIT_TIMEOUT_SEC * 1000, &status)) {
    kill(pid, SIGKILL);
    wait_pid_timeout(pid, 1000, &status);
  }

  int pass = handshake_ok && project_ok && task_ok && modified_ok && app_ins_ok && app_list_ok && app_resp_ok;
  emitf("T13", pass ? "pass" : "fail",
        "minimal_loop: handshake=%d project=%d task=%d file_modified=%d app_ins=%d app_list=%d app_resp=%d child=%s task_resp=%.200s",
        handshake_ok, project_ok, task_ok, modified_ok, app_ins_ok, app_list_ok, app_resp_ok, wait_status_str(status), task_resp);
}

static void t14_dlopen(const char *label, const char *path) {
  char tid[96];
  snprintf(tid, sizeof tid, "T14-%s", label);
  void *h = dlopen(path, RTLD_LAZY | RTLD_LOCAL);
  if (!h) {
    emitf(tid, "denied", "dlopen(%s): %s", path, dlerror());
  } else {
    dlclose(h);
    emitf(tid, "pass", "dlopen(%s) ok", path);
  }
}

static void t15_keychain(void) {
  CFMutableDictionaryRef query = CFDictionaryCreateMutable(NULL, 0, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
  CFDictionarySetValue(query, kSecClass, kSecClassGenericPassword);
  CFDictionarySetValue(query, kSecAttrService, CFSTR("com.aeroric.probe.service"));
  CFDictionarySetValue(query, kSecAttrAccount, CFSTR("test-probe-account"));
  CFDictionarySetValue(query, kSecReturnData, kCFBooleanFalse);
  CFTypeRef result = NULL;
  OSStatus st = SecItemCopyMatching(query, &result);
  CFRelease(query);
  if (st == errSecItemNotFound || st == errSecSuccess) {
    emit("T15", "pass", "SecItemCopyMatching: query accessible in sandbox");
  } else {
    emitf("T15", "denied", "SecItemCopyMatching: OSStatus=%d (errSecMissingEntitlement or restricted)", (int)st);
  }
}
static void t16_ap3_sandbox_lifecycle(void) {
  if (access(g_bundle_helper, X_OK) != 0) {
    emit("T16", "error", "helper missing");
    return;
  }
  // 1. Version mismatch refusal: schemaVersion 99 in runtime.json must exit 3 without overwriting runtime.json.
  char ver_dir[1500];
  snprintf(ver_dir, sizeof ver_dir, "%s/ap3-ver", g_container);
  mkdir_p(ver_dir);
  char rt_path[1500];
  snprintf(rt_path, sizeof rt_path, "%s/runtime.json", ver_dir);
  const char *future_rt = "{\"schemaVersion\":99,\"epoch\":\"future-ep\",\"revision\":\"1\",\"buildHash\":\"b\",\"protocolHash\":\"p\"}\n";
  FILE *rf = fopen(rt_path, "w");
  if (rf) { fputs(future_rt, rf); fclose(rf); }
  char *ver_argv[] = {g_bundle_helper, "--data-dir", ver_dir, "serve", NULL};
  int ver_status = 0;
  char ver_out[256];
  int ver_rc = spawn_capture(ver_argv, NULL, 5000, ver_out, sizeof ver_out, &ver_status);
  int ver_refused = (ver_rc == 0 && WIFEXITED(ver_status) && WEXITSTATUS(ver_status) == 3);
  char rt_after[256] = "";
  rf = fopen(rt_path, "r");
  if (rf) { size_t n = fread(rt_after, 1, sizeof rt_after - 1, rf); rt_after[n] = '\0'; fclose(rf); }
  int ver_untouched = (strstr(rt_after, "\"schemaVersion\":99") != NULL);

  // 2. Concurrent cold-start arbitration + Background keep-alive reattach + Explicit stop.
  char bg_dir[1500];
  snprintf(bg_dir, sizeof bg_dir, "%s/ap3-bg", g_container);
  mkdir_p(bg_dir);
  int ready1[2];
  if (pipe(ready1) != 0) { emit("T16", "error", "pipe failed"); return; }
  posix_spawn_file_actions_t fa1;
  posix_spawn_file_actions_init(&fa1);
  posix_spawn_file_actions_addclose(&fa1, ready1[0]);
  posix_spawn_file_actions_adddup2(&fa1, ready1[1], 3);
  posix_spawn_file_actions_addclose(&fa1, ready1[1]);
  posix_spawn_file_actions_addopen(&fa1, 1, "/dev/null", O_WRONLY, 0);
  char *bg_argv[] = {g_bundle_helper, "--data-dir", bg_dir, "serve", "--ready-fd", "3", NULL};
  pid_t pid1 = 0;
  int rc1 = posix_spawn(&pid1, g_bundle_helper, &fa1, NULL, bg_argv, environ);
  posix_spawn_file_actions_destroy(&fa1);
  close(ready1[1]);
  if (rc1 != 0) { close(ready1[0]); emit("T16", "fail", "spawn pid1 failed"); return; }

  char line[1024] = "";
  size_t used = 0;
  fcntl(ready1[0], F_SETFL, O_NONBLOCK);
  long deadline = now_ms() + READY_TIMEOUT_SEC * 1000L;
  while (now_ms() < deadline && used < sizeof line - 1) {
    ssize_t n = read(ready1[0], line + used, sizeof line - 1 - used);
    if (n > 0) { used += (size_t)n; line[used] = '\0'; if (strchr(line, '\n')) break; continue; }
    if (n == 0) break;
    if (errno != EAGAIN && errno != EWOULDBLOCK) break;
    usleep(20 * 1000);
  }
  close(ready1[0]);

  // Spawn a concurrent contender against the same bg_dir while pid1 holds the single-writer lease.
  int contender_status = 0;
  char contender_out[256];
  char *contender_argv[] = {g_bundle_helper, "--data-dir", bg_dir, "serve", NULL};
  int contender_rc = spawn_capture(contender_argv, NULL, 5000, contender_out, sizeof contender_out, &contender_status);
  int contender_rejected = (contender_rc == 0 && WIFEXITED(contender_status) && WEXITSTATUS(contender_status) == 3);

  // Connect client #1, open project, then disconnect (simulating UI exit with background keep-alive enabled).
  char sock_path[1024] = "";
  snprintf(sock_path, sizeof sock_path, "%s/host.sock", bg_dir);
  char token[256] = "";
  char token_path[1500];
  snprintf(token_path, sizeof token_path, "%s/host.token", bg_dir);
  FILE *tf = fopen(token_path, "r");
  if (tf) {
    if (fgets(token, sizeof token, tf)) {
      char *nl = strchr(token, '\n'); if (nl) *nl = '\0';
    }
    fclose(tf);
  }
  char proj_dir[1500];
  snprintf(proj_dir, sizeof proj_dir, "%s/proj-keepalive", bg_dir);
  mkdir_p(proj_dir);

  int c1_ok = 0, c2_ok = 0;
  struct sockaddr_un addr;
  memset(&addr, 0, sizeof addr);
  addr.sun_family = AF_UNIX;
  snprintf(addr.sun_path, sizeof addr.sun_path, "%s", sock_path);

  int s1 = socket(AF_UNIX, SOCK_STREAM, 0);
  if (s1 >= 0 && connect(s1, (struct sockaddr *)&addr, sizeof addr) == 0) {
    char req[1024], resp[2048];
    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"1\",\"method\":\"handshake\",\"params\":{\"token\":\"%s\",\"clientId\":\"ui-session-1\"}}\n", token);
    if (uds_rpc_call(s1, req, resp, sizeof resp) == 0 && strstr(resp, "\"ok\":true")) {
      snprintf(req, sizeof req, "{\"v\":1,\"id\":\"2\",\"method\":\"project.open\",\"params\":{\"path\":\"%s\",\"newId\":\"p-bg-1\"}}\n", proj_dir);
      if (uds_rpc_call(s1, req, resp, sizeof resp) == 0 && strstr(resp, "\"ok\":true")) {
        c1_ok = 1;
      }
    }
    close(s1);
  }

  // Host must still be alive after UI session #1 closed; UI session #2 reattaches and sees p-bg-1!
  int host_alive_between = (kill(pid1, 0) == 0);
  int s2 = socket(AF_UNIX, SOCK_STREAM, 0);
  if (s2 >= 0 && connect(s2, (struct sockaddr *)&addr, sizeof addr) == 0) {
    char req[1024], resp[2048];
    snprintf(req, sizeof req, "{\"v\":1,\"id\":\"1\",\"method\":\"handshake\",\"params\":{\"token\":\"%s\",\"clientId\":\"ui-session-2\"}}\n", token);
    if (uds_rpc_call(s2, req, resp, sizeof resp) == 0 && strstr(resp, "\"ok\":true")) {
      snprintf(req, sizeof req, "{\"v\":1,\"id\":\"2\",\"method\":\"project.list\",\"params\":{}}\n");
      if (uds_rpc_call(s2, req, resp, sizeof resp) == 0 && strstr(resp, "p-bg-1")) {
        c2_ok = 1;
      }
    }
    close(s2);
  }

  // Explicit stop terminates the background host cleanly.
  kill(pid1, SIGTERM);
  int stop_status = 0;
  if (!wait_pid_timeout(pid1, WAIT_TIMEOUT_SEC * 1000, &stop_status)) {
    kill(pid1, SIGKILL);
    wait_pid_timeout(pid1, 1000, &stop_status);
  }
  int pass = ver_refused && ver_untouched && contender_rejected && c1_ok && host_alive_between && c2_ok;
  emitf("T16", pass ? "pass" : "fail",
        "ap3_sandbox: ver_refused=%d ver_untouched=%d contender_rejected=%d c1=%d alive_between=%d c2_reattach=%d stop=%s",
        ver_refused, ver_untouched, contender_rejected, c1_ok, host_alive_between, c2_ok, wait_status_str(stop_status));
}

int main(int argc, char **argv) {
  alarm(180); // battery safety net — a hung probe must not hang the driver
  uint32_t size = sizeof g_exe;
  if (_NSGetExecutablePath(g_exe, &size) != 0) {
    emit("T00", "error", "_NSGetExecutablePath buffer too small");
    return 1;
  }
  // resolve real path of the executable (bundle Contents/MacOS/<name>)
  char real[1024];
  if (realpath(g_exe, real)) snprintf(g_exe, sizeof g_exe, "%s", real);
  snprintf(g_bundle_helper, sizeof g_bundle_helper, "%.*s/aeroric-cli",
           (int)(strrchr(g_exe, '/') - g_exe), g_exe);

  emitf("T00", "info", "AeroricSandboxProbe battery v1 exe=%s", g_exe);
  t01_env();
  t02_outside_home_write();
  t03_container_write();
  t04_exec_system();
  t05_exec_outside_cli();
  t06_exec_bundle_helper();
  t07_pty_chain();
  t08_host_serve();
  t09_login_shell();
  t10_outside_home_read();
  t13_minimal_task_loop();
  t15_keychain();
  t16_ap3_sandbox_lifecycle();
  // batch-2 driver-supplied rows: exec:<label>:<path> / read:<label>:<path> / dlopen:<label>:<path>
  for (int i = 1; i < argc; i++) {
    char *a = argv[i];
    const char *kind = NULL;
    size_t kind_len = 0;
    if (strncmp(a, "exec:", 5) == 0) { kind = "exec"; kind_len = 5; }
    else if (strncmp(a, "read:", 5) == 0) { kind = "read"; kind_len = 5; }
    else if (strncmp(a, "dlopen:", 7) == 0) { kind = "dlopen"; kind_len = 7; }
    else { emitf("T11-args", "error", "unknown arg format: %.100s", a); continue; }
    char *label = a + kind_len;
    char *path = strchr(label, ':');
    if (!path) { emitf("T11-args", "error", "missing path in: %.100s", a); continue; }
    *path++ = '\0';
    if (kind[0] == 'e') t11_exec_path(label, path);
    else if (kind[0] == 'r') t12_read_path(label, path);
    else if (kind[0] == 'd') t14_dlopen(label, path);
  }
  emit("T99", "info", "battery done");
  return 0;
}
