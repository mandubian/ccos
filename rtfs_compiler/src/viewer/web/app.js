document.addEventListener('DOMContentLoaded', () => {
    const nodes = new vis.DataSet([]);
    const edges = new vis.DataSet([]);

    const container = document.getElementById('graph-container');
    const data = { nodes, edges };
    const options = {
        layout: {
            hierarchical: {
                enabled: true,
                sortMethod: 'directed',
            },
        },
        nodes: {
            shape: 'box',
            font: {
                color: '#f0f0f0',
            },
            color: {
                border: '#00aaff',
                background: '#2a2a2a',
                highlight: {
                    border: '#00ff00',
                    background: '#3a3a3a',
                },
            },
        },
        edges: {
            arrows: 'to',
            color: '#00aaff',
        },
    };

    const network = new vis.Network(container, data, options);
    const rtfsCodeElement = document.getElementById('rtfs-code');
    const logEntriesElement = document.getElementById('log-entries');

    // WebSocket with auto-reconnect and exponential backoff
    let socket = null;
    let reconnectAttempts = 0;
    function connect() {
        socket = new WebSocket('ws://' + window.location.host + '/ws');

        socket.onopen = () => {
            reconnectAttempts = 0;
            console.log('WebSocket connection established');
            addLogEntry('Connected to CCOS server.');
        };

        socket.onmessage = (event) => {
            const message = JSON.parse(event.data);
            addLogEntry(`Received event: ${message.type}`);
            handleEvent(message);
        };

        socket.onclose = () => {
            console.log('WebSocket connection closed');
            addLogEntry('Disconnected from CCOS server.');
            scheduleReconnect();
        };

        socket.onerror = (e) => {
            console.error('WebSocket error', e);
            socket.close();
        };
    }

    function scheduleReconnect() {
        reconnectAttempts += 1;
        const delay = Math.min(30000, 1000 * Math.pow(2, reconnectAttempts));
        addLogEntry(`Reconnecting in ${delay/1000}s...`);
        setTimeout(() => connect(), delay);
    }

    connect();

    // Small UI: a form to submit new goals to POST /intent
    const intentForm = document.getElementById('intent-form');
    if (intentForm) {
        intentForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const input = document.getElementById('intent-input');
            if (!input) return;
            const goal = input.value.trim();
            if (!goal) return;
            addLogEntry(`Submitting goal: ${goal}`);
            try {
                const resp = await fetch('/intent', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ goal })
                });
                const text = await resp.text();
                addLogEntry(`Server response: ${resp.status} ${text}`);
            } catch (err) {
                addLogEntry(`Failed to submit goal: ${err}`);
            }
            input.value = '';
        });
    }

    function addLogEntry(message) {
        const entry = document.createElement('div');
        entry.className = 'log-entry';
        entry.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        logEntriesElement.appendChild(entry);
        logEntriesElement.scrollTop = logEntriesElement.scrollHeight;
    }

    function handleEvent(event) {
        switch (event.type) {
            case 'FullUpdate':
                // Merge incoming full-update into the existing graph instead of
                // replacing it wholesale. This preserves previously-reported
                // nodes/edges so the server can send incremental FullUpdate
                // messages for new sub-nodes and edges.
                if (Array.isArray(event.data.nodes)) {
                    // upsert nodes
                    event.data.nodes.forEach(n => {
                        // keep label updates and other props
                        nodes.update(n);
                    });
                }
                if (Array.isArray(event.data.edges)) {
                    // ensure edges have stable ids so updates don't duplicate
                    event.data.edges.forEach(e => {
                        const edge = Object.assign({}, e);
                        if (!edge.id) {
                            edge.id = `${edge.from}--${edge.to}`;
                        }
                        edges.update(edge);
                    });
                }
                if (typeof event.data.rtfs_code === 'string') {
                    rtfsCodeElement.textContent = event.data.rtfs_code;
                    Prism.highlightElement(rtfsCodeElement);
                }
                break;
            case 'NodeStatusChange':
                // Update the status of a single node
                nodes.update({ id: event.data.id, color: getNodeColor(event.data.status) });
                break;
        }
    }

    function getNodeColor(status) {
        switch (status) {
            case 'Pending':
                return { border: '#00aaff', background: '#2a2a2a' };
            case 'InProgress':
                return { border: '#ffff00', background: '#4a4a00' };
            case 'Success':
                return { border: '#00ff00', background: '#004a00' };
            case 'Failure':
                return { border: '#ff0000', background: '#4a0000' };
            default:
                return { border: '#cccccc', background: '#333333' };
        }
    }
});
