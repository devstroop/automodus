# WhatsApp Automation Workflows

This directory contains reusable YAML workflows for WhatsApp Web automation, extracted from the WhatsApp Automation Service (WAS) project.

## Directory Structure

```
workflows/whatsapp/
├── README.md              # This file
├── auth/                  # Authentication workflows
│   ├── qr_login.yaml     # QR code authentication
│   ├── phone_login.yaml  # Phone number authentication
│   ├── check_status.yaml # Check authentication status
│   └── logout.yaml       # Logout from WhatsApp Web
├── messaging/             # Message sending workflows
│   ├── send.yaml         # Universal send (text, media, or document)
│   ├── send_text.yaml    # Send text message only
│   ├── send_media.yaml   # Send image/video with optional caption
│   └── send_document.yaml # Send document file
└── chat/                  # Chat management workflows
    ├── get_chats.yaml    # Get list of chats from sidebar
    ├── get_messages.yaml # Get messages from a specific chat
    ├── watch_messages.yaml # Watch for new incoming messages
    └── navigate.yaml     # Navigate to a specific chat
```

## Quick Start

### 1. Authentication

Before sending messages, you must authenticate with WhatsApp Web.

**QR Code Login:**
```bash
# Get QR code for scanning
curl http://localhost:3000/whatsapp/auth/qr

# Response includes base64 QR code image
```

**Phone Number Login:**
```bash
curl -X POST http://localhost:3000/whatsapp/auth/phone \
  -H "Content-Type: application/json" \
  -d '{"phone_number": "+919876543210"}'

# Response includes verification code to enter on phone
```

**Check Status:**
```bash
curl http://localhost:3000/whatsapp/auth/status

# Returns: authorized, status, sender_id
```

### 2. Sending Messages

**Send Text Message:**
```bash
curl -X POST http://localhost:3000/whatsapp/send/text \
  -H "Content-Type: application/json" \
  -d '{
    "phone": "+919876543210",
    "message": "Hello from Automodus!"
  }'
```

**Send Image/Video:**
```bash
curl -X POST http://localhost:3000/whatsapp/send/media \
  -H "Content-Type: application/json" \
  -d '{
    "phone": "+919876543210",
    "file_path": "/path/to/image.jpg",
    "caption": "Check out this image!"
  }'
```

**Send Document:**
```bash
curl -X POST http://localhost:3000/whatsapp/send/document \
  -H "Content-Type: application/json" \
  -d '{
    "phone": "+919876543210",
    "file_path": "/path/to/document.pdf",
    "caption": "Here is the report"
  }'
```

**Universal Send (auto-detects type):**
```bash
curl -X POST http://localhost:3000/whatsapp/send \
  -H "Content-Type: application/json" \
  -d '{
    "phone": "+919876543210",
    "message": "Optional text or caption",
    "file": "/path/to/file.pdf"
  }'
```

### 3. Reading Messages

**Get Chat List:**
```bash
curl "http://localhost:3000/whatsapp/chats?limit=20"

# Returns list of chats with names, last messages, unread counts
```

**Get Messages from Chat:**
```bash
curl "http://localhost:3000/whatsapp/messages?chat_id=919876543210&limit=50"

# Or by contact name:
curl "http://localhost:3000/whatsapp/messages?chat_id=name:John%20Doe&limit=50"
```

**Watch for New Messages:**
```bash
# Call periodically to drain the message queue
curl http://localhost:3000/whatsapp/watch

# Returns any new messages since last call
```

## Workflow Parameters

### Authentication Workflows

| Workflow | Parameters | Description |
|----------|------------|-------------|
| `qr_login` | None | Returns QR code as base64 |
| `phone_login` | `phone_number` (required) | Returns verification code |
| `check_status` | None | Returns auth status |
| `logout` | None | Logs out of WhatsApp |

### Messaging Workflows

| Workflow | Parameters | Description |
|----------|------------|-------------|
| `send` | `phone` (required), `message`, `file` | Universal send |
| `send_text` | `phone` (required), `message` (required) | Text only |
| `send_media` | `phone` (required), `file_path` (required), `caption` | Image/Video |
| `send_document` | `phone` (required), `file_path` (required), `caption` | Document |

### Chat Workflows

| Workflow | Parameters | Description |
|----------|------------|-------------|
| `get_chats` | `limit` (default: 50) | List chats |
| `get_messages` | `chat_id` (required), `limit`, `load_more` | Chat messages |
| `watch_messages` | None | New incoming messages |
| `navigate` | `phone` or `name` | Open a chat |

## Browser Configuration

All WhatsApp workflows use these default browser settings:

```yaml
browser:
  headless: false          # WhatsApp requires visible window for login
  data_dir: "data/whatsapp_profile"  # Persistent session storage
  width: 1280
  height: 800
```

The `data_dir` ensures your WhatsApp session persists between workflow runs.

## CSS Selectors

These workflows use well-tested CSS selectors from the WAS project. Key selectors:

| Element | Selector |
|---------|----------|
| Auth pane | `#pane-side` |
| QR code canvas | `canvas[aria-label='Scan this QR code to link a device!']` |
| Message input | `#app #main footer div[aria-placeholder='Type a message']` |
| Send button | `button[aria-label='Send']` |
| Attach button | `button[title='Attach']` |
| Menu button | `button[title='Menu']` |

## Error Handling

All workflows include error handling:

- **on_error.screenshot**: Takes screenshot on failure
- **on_error.emit**: Emits error event for monitoring

Example error response:
```json
{
  "status": "send_failed",
  "success": false,
  "error": "Message input not found"
}
```

## Tips

1. **Rate Limiting**: WhatsApp may temporarily block accounts that send messages too quickly. Add delays between messages.

2. **Session Management**: The browser profile in `data/whatsapp_profile` keeps you logged in. Don't delete it unless you want to re-authenticate.

3. **Headless Mode**: WhatsApp Web requires a visible browser window during QR code scanning. After authentication, you may be able to use headless mode.

4. **Phone Format**: Always include country code (e.g., `+919876543210` or `919876543210`).

5. **File Paths**: Use absolute paths for file attachments, or paths relative to the workflow execution directory.

## Composition Example

You can compose these workflows together:

```yaml
# bulk_message.yaml - Send to multiple recipients
name: bulk_message
params:
  recipients:
    type: array
    required: true
  message:
    type: string
    required: true

steps:
  - action: call
    workflow: whatsapp/auth/check_status
    store_as: auth

  - action: condition
    if: "{{store.auth.authorized}} == false"
    then:
      - action: abort
        error: "Not authenticated"

  - action: loop
    items: "{{params.recipients}}"
    as: phone
    steps:
      - action: call
        workflow: whatsapp/messaging/send_text
        params:
          phone: "{{phone}}"
          message: "{{params.message}}"
      
      - action: sleep
        duration: "2s"  # Rate limiting
```

## Extracted From

These workflows were extracted from the WhatsApp Automation Service (WAS) project, specifically:

- `was/src/browser/core.rs` - WhatsAppEngine methods
- `was/src/browser/locators.rs` - CSS selectors
- `was/src/services/whatsapp/chat.rs` - Chat service implementation
- `was/src/services/auth/auth.rs` - Authentication service

The automation patterns have been converted from Rust code to declarative YAML workflows.
