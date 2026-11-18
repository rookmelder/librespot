# git

1. Windows: `winget install --id Git.Git -e` (or git-scm.com). Restart the terminal.
   Linux: your package manager (`sudo apt install git`, `sudo dnf install git`). macOS: `xcode-select --install` or `brew install git`.
2. Set your name once: `git config --global user.name "Your Name"` and `git config --global user.email "you@example.com"`.
3. Optional, to push to GitHub: `winget install --id GitHub.cli -e` (Linux/macOS: see cli.github.com), then `gh auth login`.

Why it is required: `delegate.py` uses `git status` to see exactly which files a worker changed.
