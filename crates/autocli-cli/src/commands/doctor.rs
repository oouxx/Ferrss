use colored::Colorize;
use autocli_browser::DaemonClient;
use autocli_external::is_binary_installed;
use crate::execution::env_compat;

pub async fn run_doctor() {
    println!("{}", "ferrss diagnostics".bold());
    println!();

    // 1. Check Chrome/Chromium installed
    let chrome = if cfg!(target_os = "macos") {
        is_binary_installed("google-chrome")
            || is_binary_installed("chromium")
            || std::path::Path::new("/Applications/Google Chrome.app").exists()
    } else if cfg!(target_os = "windows") {
        std::path::Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe").exists()
            || std::path::Path::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe").exists()
            || is_binary_installed("chrome")
    } else {
        // Linux
        is_binary_installed("google-chrome")
            || is_binary_installed("google-chrome-stable")
            || is_binary_installed("chromium")
            || is_binary_installed("chromium-browser")
    };
    print_check("Chrome/Chromium", chrome);

    // 2. Check daemon reachable
    let client = DaemonClient::new(
        env_compat("FERRSS_DAEMON_PORT")
            .and_then(|s| s.parse().ok())
            .unwrap_or(19925),
    );
    let daemon_running = client.is_running().await;
    print_check("Daemon running", daemon_running);

    // 3. Check extension connected
    if daemon_running {
        let ext_connected = client.is_extension_connected().await;
        print_check("Chrome extension connected", ext_connected);
    } else {
        print_check("Chrome extension connected", false);
    }

    // 4. Check external CLIs
    println!();
    println!("{}", "External CLIs:".bold());
    for name in &["gh", "docker", "kubectl"] {
        let installed = is_binary_installed(name);
        print_check(name, installed);
    }

    // 5. Check CDP endpoint
    let cdp = env_compat("FERRSS_CDP_ENDPOINT");
    if let Some(endpoint) = cdp {
        println!();
        println!("CDP endpoint: {}", endpoint);
    }

    // 6. Print adapter stats
    println!();
    println!("{}", "Adapter stats:".bold());
    // Will be filled in by main.rs passing registry info
}

fn print_check(label: &str, ok: bool) {
    if ok {
        println!("  {} {}", "✓".green(), label);
    } else {
        println!("  {} {}", "✗".red(), label);
    }
}
