//! Selector resolution for extended selector patterns
//!
//! Supports different selector prefixes:
//! - `text:` - Find by exact text content
//! - `text*:` - Find by partial text content  
//! - `role:` - Find by ARIA role and accessible name (e.g., `role:button[Submit]`)
//! - `xpath:` - XPath selector
//! - (none) - CSS selector (default)

/// Parsed selector type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorType {
    /// Standard CSS selector
    Css(String),
    /// Find by exact text content
    Text(String),
    /// Find by partial text content
    TextPartial(String),
    /// Find by ARIA role with optional accessible name: role:button[Name]
    Role { role: String, name: Option<String> },
    /// XPath selector
    XPath(String),
}

impl SelectorType {
    /// Check if this is a CSS selector (can use native find_element)
    pub fn is_css(&self) -> bool {
        matches!(self, SelectorType::Css(_))
    }

    /// Get the raw selector string for CSS selectors
    pub fn as_css(&self) -> Option<&str> {
        match self {
            SelectorType::Css(s) => Some(s),
            _ => None,
        }
    }
}

/// Parse a selector string into its type
pub fn parse_selector(selector: &str) -> SelectorType {
    if let Some(text) = selector.strip_prefix("text:") {
        SelectorType::Text(text.to_string())
    } else if let Some(text) = selector.strip_prefix("text*:") {
        SelectorType::TextPartial(text.to_string())
    } else if let Some(role_spec) = selector.strip_prefix("role:") {
        parse_role_selector(role_spec)
    } else if let Some(xpath) = selector.strip_prefix("xpath:") {
        SelectorType::XPath(xpath.to_string())
    } else {
        SelectorType::Css(selector.to_string())
    }
}

/// Parse role selector: `button[Name]` or just `button`
fn parse_role_selector(spec: &str) -> SelectorType {
    if let Some(bracket_pos) = spec.find('[') {
        let role = spec[..bracket_pos].trim().to_string();
        let name = spec[bracket_pos + 1..].trim_end_matches(']').to_string();
        SelectorType::Role {
            role,
            name: if name.is_empty() { None } else { Some(name) },
        }
    } else {
        SelectorType::Role {
            role: spec.to_string(),
            name: None,
        }
    }
}

/// Generate JavaScript to find an element by selector type
/// Returns JS that evaluates to the element or null
pub fn selector_to_js(selector: &SelectorType) -> String {
    match selector {
        SelectorType::Css(css) => {
            format!(
                r#"document.querySelector({})"#,
                serde_json::to_string(css).unwrap()
            )
        }
        SelectorType::Text(text) => {
            // Find clickable element with exact text content (trimmed)
            format!(
                r#"(function() {{
    const text = {};
    
    // Helper: check if element is clickable
    function isClickable(el) {{
        if (!el) return false;
        const tag = el.tagName?.toLowerCase();
        if (tag === 'button' || tag === 'a') return true;
        if (el.getAttribute('role') === 'button') return true;
        if (el.getAttribute('tabindex') !== null) return true;
        if (el.onclick) return true;
        return false;
    }}
    
    // Find all elements with matching innerText
    const all = document.querySelectorAll('*');
    for (const el of all) {{
        if (el.innerText && el.innerText.trim() === text) {{
            // If this element is clickable, return it
            if (isClickable(el)) return el;
            // Otherwise, find clickable ancestor
            let parent = el.parentElement;
            while (parent) {{
                if (isClickable(parent) && parent.innerText.trim() === text) {{
                    return parent;
                }}
                parent = parent.parentElement;
            }}
            // No clickable parent with exact text, return the element itself
            return el;
        }}
    }}
    return null;
}})()"#,
                serde_json::to_string(text).unwrap()
            )
        }
        SelectorType::TextPartial(text) => {
            // Find clickable element containing partial text
            format!(
                r#"(function() {{
    const text = {};
    const textLower = text.toLowerCase();
    
    // Helper: check if element is clickable
    function isClickable(el) {{
        if (!el) return false;
        const tag = el.tagName?.toLowerCase();
        if (tag === 'button' || tag === 'a') return true;
        if (el.getAttribute('role') === 'button') return true;
        if (el.getAttribute('tabindex') !== null) return true;
        if (el.onclick) return true;
        return false;
    }}
    
    // Find all elements containing the text
    const all = document.querySelectorAll('*');
    for (const el of all) {{
        if (el.innerText && el.innerText.toLowerCase().includes(textLower)) {{
            // If this element is clickable, return it
            if (isClickable(el)) return el;
            // Otherwise, find clickable ancestor
            let parent = el.parentElement;
            while (parent) {{
                if (isClickable(parent) && parent.innerText.toLowerCase().includes(textLower)) {{
                    return parent;
                }}
                parent = parent.parentElement;
            }}
            // No clickable parent, return the element itself
            return el;
        }}
    }}
    return null;
}})()"#,
                serde_json::to_string(text).unwrap()
            )
        }
        SelectorType::Role { role, name } => {
            // Find by ARIA role and optional accessible name
            match name {
                Some(name) => format!(
                    r#"(function() {{
    const role = {};
    const name = {};
    // First try explicit role attribute
    const byRole = document.querySelectorAll('[role="' + role + '"]');
    for (const el of byRole) {{
        const accName = el.getAttribute('aria-label') || el.innerText?.trim() || '';
        if (accName === name || accName.includes(name)) return el;
    }}
    // Try implicit roles (button, link, etc.)
    const implicitRoles = {{
        'button': 'button, input[type="button"], input[type="submit"], input[type="reset"]',
        'link': 'a[href]',
        'textbox': 'input[type="text"], input:not([type]), textarea',
        'checkbox': 'input[type="checkbox"]',
        'radio': 'input[type="radio"]',
        'combobox': 'select',
        'listbox': 'select[multiple]',
        'img': 'img',
        'heading': 'h1, h2, h3, h4, h5, h6'
    }};
    const implicitSelector = implicitRoles[role];
    if (implicitSelector) {{
        const elements = document.querySelectorAll(implicitSelector);
        for (const el of elements) {{
            const accName = el.getAttribute('aria-label') || el.value || el.innerText?.trim() || '';
            if (accName === name || accName.includes(name)) return el;
        }}
    }}
    return null;
}})()"#,
                    serde_json::to_string(role).unwrap(),
                    serde_json::to_string(name).unwrap()
                ),
                None => format!(
                    r#"(function() {{
    const role = {};
    // First try explicit role
    const byRole = document.querySelector('[role="' + role + '"]');
    if (byRole) return byRole;
    // Try implicit roles
    const implicitRoles = {{
        'button': 'button, input[type="button"], input[type="submit"]',
        'link': 'a[href]',
        'textbox': 'input[type="text"], input:not([type]), textarea',
        'checkbox': 'input[type="checkbox"]',
        'radio': 'input[type="radio"]',
        'combobox': 'select',
        'img': 'img',
        'heading': 'h1, h2, h3, h4, h5, h6'
    }};
    const implicitSelector = implicitRoles[role];
    if (implicitSelector) return document.querySelector(implicitSelector);
    return null;
}})()"#,
                    serde_json::to_string(role).unwrap()
                ),
            }
        }
        SelectorType::XPath(xpath) => {
            format!(
                r#"document.evaluate({}, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue"#,
                serde_json::to_string(xpath).unwrap()
            )
        }
    }
}

/// Generate JavaScript to check if element exists (returns boolean)
pub fn selector_exists_js(selector: &SelectorType) -> String {
    format!("({}) !== null", selector_to_js(selector))
}

/// Generate JavaScript to click an element
pub fn selector_click_js(selector: &SelectorType) -> String {
    format!(
        r#"(function() {{
    const el = {};
    if (!el) throw new Error('Element not found');
    el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
    el.click();
    return true;
}})()"#,
        selector_to_js(selector)
    )
}

/// Generate JavaScript to type into an element
pub fn selector_type_js(selector: &SelectorType, text: &str, clear: bool) -> String {
    let clear_js = if clear {
        "el.value = ''; el.dispatchEvent(new Event('input', { bubbles: true }));"
    } else {
        ""
    };

    format!(
        r#"(function() {{
    const el = {};
    if (!el) throw new Error('Element not found');
    el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
    el.focus();
    {}
    // Type character by character for better compatibility
    const text = {};
    for (const char of text) {{
        el.value += char;
        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    }}
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    return true;
}})()"#,
        selector_to_js(selector),
        clear_js,
        serde_json::to_string(text).unwrap()
    )
}

/// Generate JavaScript to get element's text content
pub fn selector_get_text_js(selector: &SelectorType) -> String {
    format!(
        r#"(function() {{
    const el = {};
    if (!el) return null;
    return el.innerText || el.textContent || el.value || '';
}})()"#,
        selector_to_js(selector)
    )
}

/// Generate JavaScript to get element attribute
pub fn selector_get_attribute_js(selector: &SelectorType, attr: &str) -> String {
    format!(
        r#"(function() {{
    const el = {};
    if (!el) return null;
    return el.getAttribute({});
}})()"#,
        selector_to_js(selector),
        serde_json::to_string(attr).unwrap()
    )
}

/// Generate JavaScript to hover over an element
pub fn selector_hover_js(selector: &SelectorType) -> String {
    format!(
        r#"(function() {{
    const el = {};
    if (!el) throw new Error('Element not found');
    el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
    el.dispatchEvent(new MouseEvent('mouseover', {{ bubbles: true }}));
    el.dispatchEvent(new MouseEvent('mouseenter', {{ bubbles: true }}));
    return true;
}})()"#,
        selector_to_js(selector)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_css_selector() {
        assert_eq!(
            parse_selector("button.submit"),
            SelectorType::Css("button.submit".to_string())
        );
        assert_eq!(
            parse_selector("#my-id"),
            SelectorType::Css("#my-id".to_string())
        );
    }

    #[test]
    fn test_parse_text_selector() {
        assert_eq!(
            parse_selector("text:Log in"),
            SelectorType::Text("Log in".to_string())
        );
        assert_eq!(
            parse_selector("text*:phone"),
            SelectorType::TextPartial("phone".to_string())
        );
    }

    #[test]
    fn test_parse_role_selector() {
        assert_eq!(
            parse_selector("role:button"),
            SelectorType::Role {
                role: "button".to_string(),
                name: None
            }
        );
        assert_eq!(
            parse_selector("role:button[Next]"),
            SelectorType::Role {
                role: "button".to_string(),
                name: Some("Next".to_string())
            }
        );
    }

    #[test]
    fn test_parse_xpath_selector() {
        assert_eq!(
            parse_selector("xpath://button[contains(text(),'Next')]"),
            SelectorType::XPath("//button[contains(text(),'Next')]".to_string())
        );
    }
}
