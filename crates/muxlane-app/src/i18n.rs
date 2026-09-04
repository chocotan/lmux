use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Chinese,
    English,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::Chinese, Self::English];

    pub fn id(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chinese => "中文",
            Self::English => "English",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "zh" | "zh_cn" | "zh-CN" => Some(Self::Chinese),
            "en" | "en_us" | "en-US" => Some(Self::English),
            _ => None,
        }
    }

    pub fn detect() -> Self {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find_map(|value| {
                let value = value.trim().to_ascii_lowercase();
                if value.starts_with("zh") {
                    Some(Self::Chinese)
                } else if value.starts_with("en") {
                    Some(Self::English)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }
}

const TRANSLATIONS: &[(&str, &str, &str)] = &[
    ("common.add", "添加", "Add"),
    ("common.cancel", "取消", "Cancel"),
    ("common.connect", "连接", "Connect"),
    ("common.disabled", "已关闭", "Disabled"),
    ("common.enabled", "已开启", "Enabled"),
    ("common.settings", "设置", "Settings"),
    ("dialog.add_local_project", "添加本地项目", "Add Local Project"),
    (
        "dialog.add_local_project_help",
        "输入已有项目目录；远程项目由连接机器后自动发现",
        "Enter an existing project directory. Remote projects are discovered after connecting.",
    ),
    (
        "dialog.add_remote_project",
        "在 {host} 添加项目",
        "Add Project on {host}",
    ),
    (
        "dialog.add_remote_project_help",
        "输入远端已存在的目录；不会上传或删除项目文件",
        "Enter an existing remote directory. Project files will not be uploaded or deleted.",
    ),
    ("dialog.auth_password", "用户名密码", "Password"),
    ("dialog.auth_public_key", "SSH 公钥", "SSH Key"),
    ("dialog.auth_ssh_config", "SSH 配置", "SSH Config"),
    ("dialog.connect_remote", "连接远程机器", "Connect to Remote"),
    (
        "dialog.connect_remote_help",
        "输入 SSH Host 或 ~/.ssh/config 别名；socket 自动发现",
        "Enter an SSH host or ~/.ssh/config alias. The socket is detected automatically.",
    ),
    (
        "error.create_session",
        "创建会话失败：{error}",
        "Failed to create session: {error}",
    ),
    (
        "error.create_shell",
        "创建 Shell 失败：{error}",
        "Failed to create shell: {error}",
    ),
    (
        "error.delete_remote_sessions",
        "{count} 个远端 tmux 会话未能销毁，项目仍保留",
        "Failed to destroy {count} remote tmux session(s). The project was kept.",
    ),
    (
        "error.delete_sessions_project",
        "{count} 个 tmux 会话未能销毁，项目仍保留",
        "Failed to destroy {count} tmux session(s). The project was kept.",
    ),
    (
        "error.delete_sessions_session",
        "{count} 个 tmux 会话未能销毁，会话仍保留",
        "Failed to destroy {count} tmux session(s). The session was kept.",
    ),
    (
        "error.delete_session",
        "删除会话失败：{error}",
        "Failed to delete session: {error}",
    ),
    (
        "error.invalid_local_project",
        "目录不存在或不是文件夹",
        "The directory does not exist or is not a folder.",
    ),
    (
        "error.local_project_exists",
        "这个项目已经存在",
        "This project already exists.",
    ),
    (
        "error.local_project_required",
        "请输入本地项目目录",
        "Enter a local project directory.",
    ),
    (
        "error.password_credentials_required",
        "密码连接需要用户名和密码",
        "A username and password are required.",
    ),
    (
        "error.remote_create_session",
        "远程创建会话失败：{error}",
        "Failed to create remote session: {error}",
    ),
    (
        "error.remote_machine_missing",
        "远程机器已不存在",
        "The remote machine no longer exists.",
    ),
    (
        "error.remote_not_connected",
        "远端尚未连接",
        "The remote is not connected.",
    ),
    (
        "error.remote_project_required",
        "请输入远端已有目录",
        "Enter an existing remote directory.",
    ),
    (
        "error.remote_unavailable_for_delete",
        "远端当前不可连接，未执行删除",
        "The remote is unavailable. Nothing was deleted.",
    ),
    (
        "error.ssh_target_required",
        "请输入 SSH host 或别名",
        "Enter an SSH host or alias.",
    ),
    (
        "main.state_load_failed",
        "muxlane: 无法读取状态文件 {path}: {error}\n为避免覆盖原数据，启动已中止。",
        "muxlane: failed to read state file {path}: {error}\nStartup was aborted to avoid overwriting existing data.",
    ),
    ("menu.add_remote_project", "添加远程项目…", "Add Remote Project…"),
    ("menu.confirm_delete", "确认删除", "Delete"),
    ("menu.delete_project", "删除项目", "Delete Project"),
    ("menu.delete_project_ellipsis", "删除项目…", "Delete Project…"),
    ("menu.delete_remote_machine", "删除远程机器…", "Delete Remote…"),
    (
        "menu.delete_remote_machine_title",
        "删除远程机器连接",
        "Delete Remote Connection",
    ),
    (
        "menu.delete_remote_project",
        "删除远程项目…",
        "Delete Remote Project…",
    ),
    (
        "menu.delete_remote_session",
        "删除远程会话",
        "Delete Remote Session",
    ),
    ("menu.delete_session", "删除会话", "Delete Session"),
    ("menu.deleting", "删除中…", "Deleting…"),
    ("menu.reconnect", "重新连接", "Reconnect"),
    (
        "menu.reinstall_remote",
        "重新部署 / 安装远端 Muxlane…",
        "Redeploy / Install Remote Muxlane…",
    ),
    (
        "menu.upgrade_remote",
        "更新远端 Muxlane…",
        "Update Remote Muxlane…",
    ),
    (
        "confirm.delete_project_copy",
        "将结束 {count} 个 muxlane tmux 会话。项目文件和用户默认 tmux 不会删除。",
        "This will end {count} muxlane tmux session(s). Project files and user tmux sessions will not be deleted.",
    ),
    (
        "confirm.delete_remote_copy",
        "只删除本地连接、镜像和 tunnel；目标机器上的项目、session 与 tmux 全部保留。",
        "Only the local connection, mirror, and tunnel will be deleted. Projects, sessions, and tmux processes on the remote machine will be kept.",
    ),
    ("bootstrap.action.install", "安装并启动", "Install and Start"),
    ("bootstrap.action.start", "启动并重连", "Start and Reconnect"),
    ("bootstrap.action.upgrade", "更新并重启", "Update and Restart"),
    (
        "bootstrap.confirm_title",
        "{action}远端 Muxlane",
        "{action} Remote Muxlane",
    ),
    (
        "bootstrap.description.install",
        "SSH 已连接到 {host}。将使用当前认证方式上传当前 Muxlane 并启动 headless 进程。",
        "SSH is connected to {host}. The current authentication method will be used to upload Muxlane and start the headless process.",
    ),
    (
        "bootstrap.description.start",
        "SSH 已连接到 {host}。将使用当前认证方式启动 headless 进程。",
        "SSH is connected to {host}. The current authentication method will be used to start the headless process.",
    ),
    (
        "bootstrap.description.upgrade",
        "SSH 已连接到 {host}。将使用当前认证方式上传新版本并重启 headless 进程。",
        "SSH is connected to {host}. The current authentication method will be used to upload the new version and restart the headless process.",
    ),
    ("bootstrap.install", "安装…", "Install…"),
    ("bootstrap.phase.install", "安装", "Installing"),
    ("bootstrap.phase.restart", "重启服务", "Restarting Service"),
    ("bootstrap.phase.upload", "上传二进制", "Uploading Binary"),
    ("bootstrap.start", "启动…", "Start…"),
    ("bootstrap.update", "更新…", "Update…"),
    ("notification.center", "通知中心", "Notifications"),
    ("notification.clear", "清空", "Clear"),
    (
        "notification.click_to_open",
        "点击直达终端",
        "Click to open terminal",
    ),
    ("notification.empty", "暂无通知", "No notifications"),
    (
        "notification.title_done",
        "{machine} · {project} 任务完成",
        "{machine} · {project} Task completed",
    ),
    (
        "notification.title_input",
        "{machine} · {project} 等待输入",
        "{machine} · {project} Input required",
    ),
    (
        "pane.select_agent",
        "从左侧选择 agent 打开 tab",
        "Select an agent from the sidebar to open a tab",
    ),
    (
        "palette.clear_notifications",
        "清空所有通知",
        "Clear All Notifications",
    ),
    ("palette.close_split", "关闭当前分屏", "Close Current Pane"),
    (
        "palette.connect_remote",
        "连接远程开发机…",
        "Connect to Remote…",
    ),
    ("palette.horizontal_split", "水平分屏", "Split Right"),
    (
        "palette.maximize",
        "最大化 / 还原当前面板",
        "Maximize / Restore Current Pane",
    ),
    ("palette.new", "新建 {name}", "New {name}"),
    ("palette.no_results", "无匹配结果", "No Results"),
    (
        "palette.placeholder",
        "输入命令、项目名或 Agent 名…",
        "Type a command, project, or agent name…",
    ),
    ("palette.toggle_dark", "切换为深色模式", "Switch to Dark Mode"),
    (
        "palette.toggle_light",
        "切换为浅色模式",
        "Switch to Light Mode",
    ),
    ("palette.vertical_split", "垂直分屏", "Split Down"),
    ("placeholder.password", "密码", "Password"),
    (
        "placeholder.private_key",
        "私钥路径（可选）",
        "Private key path (optional)",
    ),
    (
        "placeholder.remote_target",
        "nuc 或 192.168.1.20",
        "nuc or 192.168.1.20",
    ),
    (
        "placeholder.username",
        "用户名（可选）",
        "Username (optional)",
    ),
    ("relative.days_ago", "{count}天前", "{count}d ago"),
    ("relative.hours_ago", "{count}小时前", "{count}h ago"),
    ("relative.just_now", "刚刚", "just now"),
    ("relative.minutes_ago", "{count}分钟前", "{count}m ago"),
    ("relative.seconds_ago", "{count}秒前", "{count}s ago"),
    ("sidebar.connect_remote", "连接远程机器", "Connect to Remote"),
    ("sidebar.notifications", "通知", "Notifications"),
    ("status.auth_failed", "认证失败", "Authentication Failed"),
    ("status.connected", "已连接", "Connected"),
    ("status.connecting", "连接中", "Connecting"),
    ("status.disconnected", "已断开", "Disconnected"),
    ("status.idle", "空闲", "Idle"),
    ("status.input_required", "等待输入", "Input Required"),
    ("status.needs_install", "未安装", "Not Installed"),
    ("status.needs_start", "未启动", "Not Running"),
    ("status.needs_update", "需要更新", "Update Required"),
    ("status.offline", "离线", "Offline"),
    ("status.task_completed", "任务完成", "Task Completed"),
    ("status.task_completed_body", "任务已完成", "Task completed"),
    ("status.working", "执行中", "Working"),
    ("status.remote_ssh_probe", "SSH 探测", "Checking SSH"),
    ("status.remote_subscribe", "订阅状态", "Subscribing"),
    ("status.remote_tunnel", "建立 tunnel", "Opening Tunnel"),
    ("settings.interface_theme", "界面主题", "Interface Theme"),
    ("settings.language", "语言", "Language"),
    (
        "settings.notification_sound",
        "通知声音",
        "Notification Sound",
    ),
    (
        "settings.osc52",
        "允许终端写入剪贴板 (OSC52)",
        "Allow Terminal Clipboard Writes (OSC52)",
    ),
    ("settings.terminal_font", "终端字体", "Terminal Font"),
    ("settings.theme", "主题", "Theme"),
    (
        "terminal.attaching",
        "正在 attach 远程终端…",
        "Attaching to remote terminal…",
    ),
    (
        "terminal.reconnecting",
        "镜像流断开，正在重连…",
        "Mirror stream disconnected. Reconnecting…",
    ),
    ("theme.dark", "墨渊", "Ink"),
    ("theme.jade", "竹青", "Jade"),
    ("theme.light", "雾白瓷", "Porcelain"),
    ("theme.one_dark", "代码墨", "One Dark"),
    ("theme.paper", "纸暖", "Paper Warm"),
    ("theme.sakura", "樱粉", "Sakura"),
    ("theme.sky", "霁蓝", "Clear Sky"),
    ("theme.synthwave", "夜霓", "Synthwave"),
];

pub fn text(language: Language, key: &str) -> &str {
    for &(candidate, chinese, english) in TRANSLATIONS {
        if candidate == key {
            return match language {
                Language::Chinese => chinese,
                Language::English => english,
            };
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_ids_translations_and_fallback_are_stable() {
        assert_eq!(Language::from_id("zh"), Some(Language::Chinese));
        assert_eq!(Language::from_id("en-US"), Some(Language::English));
        assert_eq!(text(Language::English, "common.settings"), "Settings");
        assert_eq!(text(Language::Chinese, "missing.key"), "missing.key");
    }
}
