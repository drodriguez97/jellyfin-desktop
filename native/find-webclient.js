// Turn what the user typed into a URL when they leave off http:// or https://.
// - A public domain name (jellyfin.example.com) is almost always behind a
//   reverse proxy with TLS, so use https.
// - A bare IP or local name (192.168.1.50, myserver, nas.local) is almost
//   always Jellyfin itself, so use http on its default port 8096.
// - Anything with an explicit port is left on http, as before.
function normalizeServerAddress(server) {
    server = server.trim().replace(/\/+$/, '');
    if (/^https?:\/\//i.test(server)) {
        return server;
    }

    const host = server.split('/')[0];
    const hasPort = /:\d+$/.test(host);
    const isIPv4 = /^\d{1,3}(\.\d{1,3}){3}$/.test(host);
    const isLocalName = !host.includes('.') || /\.(local|lan|home|internal|localdomain)$/i.test(host);

    if (hasPort) {
        return "http://" + server;
    }
    if (isIPv4 || isLocalName) {
        return "http://" + host + ":8096" + server.slice(host.length);
    }
    return "https://" + server;
}

function showStatus(text) {
    const status = document.getElementById('status');
    status.textContent = text || '';
    status.style.display = text ? 'block' : 'none';
}

function describeFailure(server, error) {
    let msg = "Can't reach " + server + (error ? " (" + error + ")" : "") + ". Retrying every few seconds.";
    try {
        const u = new URL(server);
        if (!u.port && u.protocol === 'http:') {
            msg += " Try https://" + u.hostname + " or " + u.hostname + ":8096.";
        } else if (u.protocol === 'https:' && !u.port) {
            msg += " If this server doesn't use https, try http://" + u.hostname + ":8096.";
        }
    } catch (e) {}
    return msg + " Press Esc to cancel and edit the address.";
}

async function tryConnect(server) {
    try {
        server = normalizeServerAddress(server);

        console.log("Checking connectivity to:", server);
        showStatus("Connecting to " + server + "\u2026 Press Esc to cancel.");

        const resolvedUrl = await window.jmpCheckServerConnectivity(server, (error) => {
            showStatus(describeFailure(server, error));
        });
        console.log("Server connectivity check passed");
        console.log("Resolved URL:", resolvedUrl);

        // Save original URL but navigate to fully-resolved redirect
        window.jmpInfo.settings.main.userWebClient = server;

        // Navigation will clean up handlers, but do it explicitly
        window.location = resolvedUrl;

        return true;
    } catch (e) {
        console.error("Server connectivity check failed:", e);
        showStatus('');
        return false;
    }
}

let isConnecting = false;

const updateButtonState = () => {
    const address = document.getElementById('address');
    const button = document.getElementById('connect-button');
    const hasValue = address.value.trim().length > 0;

    if (!isConnecting) {
        button.disabled = !hasValue;
    }
};

const cancelOnEscape = (e) => {
    if (isConnecting && e.key === 'Escape') {
        cancelConnection();
    }
};

const startConnecting = async () => {
    const address = document.getElementById('address');
    const title = document.getElementById('title');
    const spinner = document.getElementById('spinner');
    const button = document.getElementById('connect-button');
    const server = address.value;

    isConnecting = true;
    title.textContent = '';
    title.style.visibility = 'hidden';
    address.classList.add('connecting');
    address.style.visibility = 'hidden';
    address.disabled = true;
    spinner.style.display = 'block';
    button.style.visibility = 'hidden';
    document.addEventListener('keydown', cancelOnEscape);

    // C++ handles retries, just wait for result
    const connected = await tryConnect(server);

    if (!connected) {
        isConnecting = false;
        title.textContent = document.getElementById('title').getAttribute('data-original-text');
        title.style.visibility = 'visible';
        address.classList.remove('connecting');
        address.style.visibility = 'visible';
        address.disabled = false;
        spinner.style.display = 'none';
        button.style.visibility = 'visible';
        document.removeEventListener('keydown', cancelOnEscape);
        updateButtonState();
    }
};

const cancelConnection = () => {
    if (!isConnecting) return;

    console.log("Cancelling connection");
    isConnecting = false;
    showStatus('');

    // Cancel C++ connectivity check and abort JS promise
    if (window.api && window.api.system) {
        window.api.system.cancelServerConnectivity();
    }
    if (window.jmpCheckServerConnectivity.abort) {
        window.jmpCheckServerConnectivity.abort();
    }

    const address = document.getElementById('address');
    const title = document.getElementById('title');
    const spinner = document.getElementById('spinner');
    const button = document.getElementById('connect-button');

    title.textContent = document.getElementById('title').getAttribute('data-original-text');
    title.style.visibility = 'visible';
    address.classList.remove('connecting');
    address.style.visibility = 'visible';
    address.disabled = false;
    spinner.style.display = 'none';
    button.style.visibility = 'visible';
    document.removeEventListener('keydown', cancelOnEscape);
    updateButtonState();
};

// Button click handler
document.getElementById('connect-button').addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();

    if (!e.target.disabled) {
        startConnecting();
    }
});

// Form submit handler
document.getElementById('connect-form').addEventListener('submit', (e) => {
    e.preventDefault();
    if (!isConnecting) {
        startConnecting();
    }
});

// Input change handler
document.getElementById('address').addEventListener('input', updateButtonState);


// Enter key handler
document.addEventListener('keydown', (e) => {
    const address = document.getElementById('address');
    if (e.key === 'Enter' && !isConnecting && !address.disabled && address.value.trim()) {
        e.preventDefault();
        startConnecting();
    }
});

// Auto-connect on load
(async () => {
    console.log('Auto-connect: starting');

    await window.apiPromise;

    const savedServer = window.jmpInfo.settings.main.userWebClient;
    console.log('Auto-connect: savedServer =', savedServer);

    if (savedServer) {
        console.log('Auto-connect: checking saved server', savedServer);

        const address = document.getElementById('address');
        const title = document.getElementById('title');
        const spinner = document.getElementById('spinner');
        const button = document.getElementById('connect-button');

        // Set address value for potential display later
        address.value = savedServer;

        // Show connecting UI
        isConnecting = true;
        title.textContent = '';
        title.style.visibility = 'hidden';
        address.classList.add('connecting');
        address.style.visibility = 'hidden';
        address.disabled = true;
        spinner.style.display = 'block';
        button.style.visibility = 'hidden';
        document.addEventListener('keydown', cancelOnEscape);

        // C++ handles retries, just wait for result
        const connected = await tryConnect(savedServer);

        if (!connected) {
            // User cancelled or error - show UI
            isConnecting = false;
            title.textContent = document.getElementById('title').getAttribute('data-original-text');
            title.style.visibility = 'visible';
            address.classList.remove('connecting');
            address.style.visibility = 'visible';
            address.disabled = false;
            spinner.style.display = 'none';
            button.style.visibility = 'visible';
            document.removeEventListener('keydown', cancelOnEscape);
            address.focus();
            updateButtonState();
        }
    } else {
        const title = document.getElementById('title');
        const address = document.getElementById('address');
        const button = document.getElementById('connect-button');

        title.style.visibility = 'visible';
        address.style.visibility = 'visible';
        button.style.visibility = 'visible';
        address.focus();
        updateButtonState();
    }
})();
