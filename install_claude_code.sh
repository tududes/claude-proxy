#!/bin/bash

### Thanks to Z.AI for creating this script! Original version, which uses z.ai, here: https://cdn.bigmodel.cn/install/claude_code_zai_env.sh

set -euo pipefail

# ========================
#       Define Constants
# ========================
SCRIPT_NAME=$(basename "$0")
NODE_MIN_VERSION=18
NODE_INSTALL_VERSION=22
NVM_VERSION="v0.40.3"
CLAUDE_PACKAGE="@anthropic-ai/claude-code"
CONFIG_DIR="$HOME/.claude"
CONFIG_FILE="$CONFIG_DIR/settings.json"
API_BASE_URL="https://claude.chutes.ai"
API_KEY_URL="https://chutes.ai/app/api"
API_TIMEOUT_MS=6000000

# ========================
#       Functions
# ========================

log_info() {
    echo "ðŸ”¹ $*"
}

log_success() {
    echo "âœ… $*"
}

log_error() {
    echo "âŒ $*" >&2
}

ensure_dir_exists() {
    local dir="$1"
    if [ ! -d "$dir" ]; then
        mkdir -p "$dir" || {
            log_error "Failed to create directory: $dir"
            exit 1
        }
    fi
}

# ========================
#     Node.js Installation
# ========================

install_nodejs() {
    local platform=$(uname -s)

    case "$platform" in
        Linux|Darwin)
            log_info "Installing Node.js on $platform..."

            # Install nvm
            log_info "Installing nvm ($NVM_VERSION)..."
            curl -s https://raw.githubusercontent.com/nvm-sh/nvm/"$NVM_VERSION"/install.sh | bash

            # Load nvm
            log_info "Loading nvm environment..."
            \. "$HOME/.nvm/nvm.sh"

            # Install Node.js
            log_info "Installing Node.js $NODE_INSTALL_VERSION..."
            nvm install "$NODE_INSTALL_VERSION"

            # Verify installation
            node -v &>/dev/null || {
                log_error "Node.js installation failed"
                exit 1
            }
            log_success "Node.js installed: $(node -v)"
            log_success "npm version: $(npm -v)"
            ;;
        *)
            log_error "Unsupported platform: $platform"
            exit 1
            ;;
    esac
}

# ========================
#     Node.js Check
# ========================

check_nodejs() {
    if command -v node &>/dev/null; then
        current_version=$(node -v | sed 's/v//')
        major_version=$(echo "$current_version" | cut -d. -f1)

        if [ "$major_version" -ge "$NODE_MIN_VERSION" ]; then
            log_success "Node.js is already installed: v$current_version"
            return 0
        else
            log_info "Node.js v$current_version is installed but version < $NODE_MIN_VERSION. Upgrading..."
            install_nodejs
        fi
    else
        log_info "Node.js not found. Installing..."
        install_nodejs
    fi
}

# ========================
#     Claude Code Installation
# ========================

install_claude_code() {
    if command -v claude &>/dev/null; then
        log_success "Claude Code is already installed: $(claude --version)"
    else
        log_info "Installing Claude Code..."
        npm install -g "$CLAUDE_PACKAGE" || {
            log_error "Failed to install claude-code"
            exit 1
        }
        log_success "Claude Code installed successfully"
    fi
}

configure_claude_json(){
  node --eval '
      const os = require("os");
      const fs = require("fs");
      const path = require("path");

      const homeDir = os.homedir();
      const filePath = path.join(homeDir, ".claude.json");
      if (fs.existsSync(filePath)) {
          const content = JSON.parse(fs.readFileSync(filePath, "utf-8"));
          fs.writeFileSync(filePath, JSON.stringify({ ...content, hasCompletedOnboarding: true }, null, 2), "utf-8");
      } else {
          fs.writeFileSync(filePath, JSON.stringify({ hasCompletedOnboarding: true }, null, 2), "utf-8");
      }'
}

# ========================
#     Model Selection
# ========================

select_model() {
    local api_key="$1"
    
    log_info "Fetching available models from $API_BASE_URL..."
    
    # Fetch models from API
    local models_response
    models_response=$(curl -s -H "Authorization: Bearer $api_key" "$API_BASE_URL/v1/models" 2>/dev/null)
    
    if [ $? -ne 0 ] || [ -z "$models_response" ]; then
        log_error "Failed to fetch models from API"
        echo "   Using default model: deepseek-ai/DeepSeek-R1"
        echo "deepseek-ai/DeepSeek-R1"
        return
    fi
    
    # Parse model IDs using node
    local models
    models=$(echo "$models_response" | node --eval '
        const data = JSON.parse(require("fs").readFileSync(0, "utf-8"));
        if (data.data && Array.isArray(data.data)) {
            data.data.forEach((model, idx) => {
                console.log((idx + 1) + "|" + model.id);
            });
        }
    ' 2>/dev/null)
    
    if [ -z "$models" ]; then
        log_error "No models found in API response"
        echo "   Using default model: deepseek-ai/DeepSeek-R1"
        echo "deepseek-ai/DeepSeek-R1"
        return
    fi
    
    # Display models
    echo ""
    log_info "Available models:"
    echo "$models" | while IFS='|' read -r num model_id; do
        printf "   %2s) %s\n" "$num" "$model_id"
    done
    echo ""
    
    # Get user selection
    local total_models
    total_models=$(echo "$models" | wc -l)
    
    while true; do
        read -p "🎯 Select a model (1-$total_models) [default: 1]: " selection
        selection=${selection:-1}
        
        if [[ "$selection" =~ ^[0-9]+$ ]] && [ "$selection" -ge 1 ] && [ "$selection" -le "$total_models" ]; then
            local selected_model
            selected_model=$(echo "$models" | sed -n "${selection}p" | cut -d'|' -f2)
            echo "$selected_model"
            return
        else
            log_error "Invalid selection. Please enter a number between 1 and $total_models"
        fi
    done
}

# ========================
#     API Key Configuration
# ========================

configure_claude() {
    log_info "Configuring Claude Code..."
    echo "   You can get your API key from: $API_KEY_URL"
    read -s -p "🔑 Please enter your chutes.ai API key: " api_key
    echo

    if [ -z "$api_key" ]; then
        log_error "API key cannot be empty. Please run the script again."
        exit 1
    fi
    
    # Select model interactively
    local selected_model
    selected_model=$(select_model "$api_key")
    log_success "Selected model: $selected_model"

    ensure_dir_exists "$CONFIG_DIR"

    # Write settings.json
    node --eval '
      const os = require("os");
      const fs = require("fs");
      const path = require("path");

      const homeDir = os.homedir();
      const filePath = path.join(homeDir, ".claude", "settings.json");
      const apiKey = "'"$api_key"'";
      const selectedModel = "'"$selected_model"'";
      const apiBaseUrl = "'"$API_BASE_URL"'";
      const apiTimeout = "'"$API_TIMEOUT_MS"'";

      const content = fs.existsSync(filePath)
          ? JSON.parse(fs.readFileSync(filePath, "utf-8"))
          : {};

      fs.writeFileSync(filePath, JSON.stringify({
          ...content,
          model: selectedModel,
          alwaysThinkingEnabled: true,
          env: {
              ANTHROPIC_AUTH_TOKEN: apiKey,
              ANTHROPIC_BASE_URL: apiBaseUrl,
              API_TIMEOUT_MS: apiTimeout,
              CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
              ANTHROPIC_DEFAULT_HAIKU_MODEL: selectedModel,
              ANTHROPIC_DEFAULT_SONNET_MODEL: selectedModel,
              ANTHROPIC_DEFAULT_OPUS_MODEL: selectedModel,
              CLAUDE_CODE_SUBAGENT_MODEL: selectedModel,
              ANTHROPIC_SMALL_FAST_MODEL: selectedModel
          }
      }, null, 2), "utf-8");
    ' || {
        log_error "Failed to write settings.json"
        exit 1
    }

    log_success "Claude Code configured successfully"
}

# ========================
#        Main
# ========================

main() {
    echo "ðŸš€ Starting $SCRIPT_NAME"

    check_nodejs
    install_claude_code
    configure_claude_json
    configure_claude

    echo ""
    log_success "ðŸŽ‰ Installation completed successfully!"
    echo ""
    echo "ðŸš€ You can now start using Claude Code with:"
    echo "   claude"
}

main "$@"