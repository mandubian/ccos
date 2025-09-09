document.addEventListener('DOMContentLoaded', () => {
    const nodes = new vis.DataSet([]);
    const edges = new vis.DataSet([]);

    const container = document.getElementById('graph-visualization');
    console.log('Graph visualization container:', container);
    console.log('Container dimensions:', container ? { width: container.offsetWidth, height: container.offsetHeight } : 'Container not found');
    
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
        interaction: {
            selectConnectedEdges: false,
        },
    };

    const network = new vis.Network(container, data, options);
    console.log('Vis.js network initialized:', network);
    const rtfsCodeElement = document.getElementById('rtfs-code');
    const logEntriesElement = document.getElementById('log-entries');
    const goalStatusElement = document.getElementById('goal-status');
    const graphStatsElement = document.getElementById('graph-stats');
    const selectedIntentInfoElement = document.getElementById('selected-intent-info');

    // State management
    let currentGraphId = null;
    let selectedIntentId = null;
    let intentNodes = new Map(); // id -> node data
    let intentEdges = new Map(); // id -> edge data

    // WebSocket with auto-reconnect and exponential backoff
    let socket = null;
    let reconnectAttempts = 0;
    function connect() {
        socket = new WebSocket('ws://localhost:3001/ws');

        socket.onopen = () => {
            reconnectAttempts = 0;
            console.log('WebSocket connection established');
            addLogEntry('Connected to CCOS server.');
            updateGoalStatus('Connected to CCOS server.');
        };

        socket.onmessage = (event) => {
            const message = JSON.parse(event.data);
            console.log('Received WebSocket message:', message);
            addLogEntry(`Received event: ${message.type}`);
            handleEvent(message);
        };

        socket.onclose = () => {
            console.log('WebSocket connection closed');
            addLogEntry('Disconnected from CCOS server.');
            updateGoalStatus('Disconnected from CCOS server.');
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
        updateGoalStatus(`Reconnecting in ${delay/1000}s...`);
        setTimeout(() => connect(), delay);
    }

    connect();

    // Goal form submission
    const goalForm = document.getElementById('goal-form');
    if (goalForm) {
        goalForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const input = document.getElementById('goal-input');
            if (!input) return;
            const goal = input.value.trim();
            if (!goal) return;

            addLogEntry(`Generating graph for goal: ${goal}`);
            updateGoalStatus('Generating intent graph...');

            try {
                const resp = await fetch('/generate-graph', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ goal })
                });

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`Graph generation started: ${result.message}`);
                    updateGoalStatus('Graph generation in progress...');
                } else {
                    const text = await resp.text();
                    addLogEntry(`Failed to generate graph: ${resp.status} ${text}`);
                    updateGoalStatus('Failed to generate graph.');
                }
            } catch (err) {
                addLogEntry(`Failed to submit goal: ${err}`);
                updateGoalStatus('Error submitting goal.');
            }
        });
    }

    // Action buttons
    const generatePlansBtn = document.getElementById('generate-plans-btn');
    const executeBtn = document.getElementById('execute-btn');
    const clearBtn = document.getElementById('clear-btn');

    if (generatePlansBtn) {
        generatePlansBtn.addEventListener('click', async () => {
            addLogEntry('Generating plans for all intents...');
            updateGoalStatus('Generating plans...');

            try {
                const resp = await fetch('/generate-plans', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({})
                });

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`Plan generation started: ${result.message}`);
                    updateGoalStatus('Plan generation in progress...');
                } else {
                    const text = await resp.text();
                    addLogEntry(`Failed to generate plans: ${resp.status} ${text}`);
                    updateGoalStatus('Failed to generate plans.');
                }
            } catch (err) {
                addLogEntry(`Failed to generate plans: ${err}`);
                updateGoalStatus('Error generating plans.');
            }
        });
    }

    if (executeBtn) {
        executeBtn.addEventListener('click', async () => {
            addLogEntry('Executing plans...');
            updateGoalStatus('Executing plans...');

            try {
                const resp = await fetch('/execute', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({})
                });

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`Execution started: ${result.message}`);
                    updateGoalStatus('Execution in progress...');
                } else {
                    const text = await resp.text();
                    addLogEntry(`Failed to execute: ${resp.status} ${text}`);
                    updateGoalStatus('Failed to execute.');
                }
            } catch (err) {
                addLogEntry(`Failed to execute: ${err}`);
                updateGoalStatus('Error executing plans.');
            }
        });
    }

    if (clearBtn) {
        clearBtn.addEventListener('click', () => {
            // Clear graph
            nodes.clear();
            edges.clear();
            intentNodes.clear();
            intentEdges.clear();

            // Clear UI
            rtfsCodeElement.textContent = '';
            selectedIntentInfoElement.textContent = 'Select an intent to view details';
            graphStatsElement.textContent = 'No graph generated yet';
            updateGoalStatus('Ready to generate intent graph...');

            // Reset state
            currentGraphId = null;
            selectedIntentId = null;

            // Disable buttons
            generatePlansBtn.disabled = true;
            executeBtn.disabled = true;

            addLogEntry('Cleared all data.');
        });
    }

    // Network click handler for intent selection
    network.on('selectNode', (params) => {
        if (params.nodes.length > 0) {
            const nodeId = params.nodes[0];
            selectIntent(nodeId);
        }
    });

    function selectIntent(intentId) {
        selectedIntentId = intentId;
        const node = intentNodes.get(intentId);
        if (node) {
            selectedIntentInfoElement.innerHTML = `
                <strong>ID:</strong> ${node.id}<br>
                <strong>Name:</strong> ${node.label || 'N/A'}<br>
                <strong>Status:</strong> <span class="status-${node.status || 'pending'}">${node.status || 'pending'}</span><br>
                <strong>Goal:</strong> ${node.goal || 'N/A'}<br>
                <strong>Created:</strong> ${node.created_at ? new Date(node.created_at * 1000).toLocaleString() : 'N/A'}
            `;
            addLogEntry(`Selected intent: ${node.label || node.id}`);
        }
    }

    function updateGoalStatus(message) {
        goalStatusElement.textContent = message;
    }

    function updateGraphStats() {
        const nodeCount = nodes.length;
        const edgeCount = edges.length;
        graphStatsElement.textContent = `${nodeCount} intents, ${edgeCount} relationships`;
    }

    function addLogEntry(message) {
        const entry = document.createElement('div');
        entry.className = 'log-entry';
        entry.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        logEntriesElement.appendChild(entry);
        logEntriesElement.scrollTop = logEntriesElement.scrollHeight;
    }

    function handleEvent(event) {
        console.log('handleEvent called with event:', event);
        switch (event.type) {
            case 'FullUpdate':
                handleFullUpdate(event.data);
                break;
            case 'NodeStatusChange':
                handleNodeStatusChange(event.data);
                break;
            case 'StepLog':
                handleStepLog(event.data);
                break;
            case 'GraphGenerated':
                console.log('Routing to handleGraphGenerated');
                handleGraphGenerated(event.data);
                break;
            case 'PlanGenerated':
                handlePlanGenerated(event.data);
                break;
            case 'ReadyForNext':
                handleReadyForNext(event.data);
                break;
        }
    }

    function handleFullUpdate(data) {
        if (Array.isArray(data.nodes)) {
            data.nodes.forEach(n => {
                intentNodes.set(n.id, n);
                nodes.update(n);
            });
        }
        if (Array.isArray(data.edges)) {
            data.edges.forEach(e => {
                const edgeId = `${e.from}--${e.to}`;
                intentEdges.set(edgeId, e);
                edges.update({ ...e, id: edgeId });
            });
        }
        if (typeof data.rtfs_code === 'string') {
            rtfsCodeElement.textContent = data.rtfs_code;
            Prism.highlightElement(rtfsCodeElement);
        }
        updateGraphStats();
        addLogEntry('Graph updated with new data.');
    }

    function handleNodeStatusChange(data) {
        const node = intentNodes.get(data.id);
        if (node) {
            node.status = data.status;
            nodes.update({
                id: data.id,
                color: getNodeColor(data.status),
                title: `${node.label}\nStatus: ${data.status}`
            });
            addLogEntry(`Intent ${data.id} status changed to: ${data.status}`);
        }
    }

    function handleStepLog(data) {
        addLogEntry(`[${data.step}] ${data.status}: ${data.message}`);
        if (data.details) {
            addLogEntry(`Details: ${JSON.stringify(data.details, null, 2)}`);
        }
    }

    function handleGraphGenerated(data) {
        console.log('handleGraphGenerated called with data:', data);
        currentGraphId = data.root_id;
        generatePlansBtn.disabled = false;
        updateGoalStatus('Graph generated successfully. Ready to generate plans.');
        addLogEntry(`Graph generated with root ID: ${data.root_id}`);

        // Clear existing graph data
        nodes.clear();
        edges.clear();
        intentNodes.clear();
        intentEdges.clear();

        // Add nodes to the graph
        if (Array.isArray(data.nodes)) {
            console.log('Adding nodes:', data.nodes);
            data.nodes.forEach(node => {
                intentNodes.set(node.id, node);
                const nodeData = {
                    id: node.id,
                    label: node.label || node.id,
                    color: getNodeColor(node.status || 'pending'),
                    title: `${node.label || node.id}\nStatus: ${node.status || 'pending'}\nType: ${node.type || 'unknown'}`
                };
                console.log('Adding node to vis.js:', nodeData);
                nodes.add(nodeData);
            });
        }

        // Add edges to the graph
        if (Array.isArray(data.edges)) {
            console.log('Adding edges:', data.edges);
            data.edges.forEach(edge => {
                const edgeId = `${edge.source}--${edge.target}`;
                intentEdges.set(edgeId, edge);
                const edgeData = {
                    id: edgeId,
                    from: edge.source,
                    to: edge.target,
                    label: edge.type || '',
                    arrows: 'to',
                    color: '#00aaff'
                };
                console.log('Adding edge to vis.js:', edgeData);
                edges.add(edgeData);
            });
        }

        // Force a redraw of the network
        console.log('Forcing network redraw...');
        network.redraw();
        network.fit();

        updateGraphStats();
        addLogEntry(`Rendered graph with ${data.nodes ? data.nodes.length : 0} nodes and ${data.edges ? data.edges.length : 0} edges`);
    }

    function handlePlanGenerated(data) {
        executeBtn.disabled = false;
        updateGoalStatus('Plans generated successfully. Ready to execute.');
        addLogEntry(`Plan generated for intent ${data.intent_id}: ${data.plan_id}`);
        if (data.rtfs_code) {
            rtfsCodeElement.textContent = data.rtfs_code;
            Prism.highlightElement(rtfsCodeElement);
        }
    }

    function handleReadyForNext(data) {
        updateGoalStatus(`Ready for next step: ${data.next_step}`);
        addLogEntry(`Ready for next step: ${data.next_step}`);
    }

    function getNodeColor(status) {
        switch (status) {
            case 'Active':
                return { border: '#00aaff', background: '#2a2a2a' };
            case 'Executing':
                return { border: '#ffff00', background: '#4a4a00' };
            case 'Completed':
                return { border: '#00ff00', background: '#004a00' };
            case 'Failed':
                return { border: '#ff0000', background: '#4a0000' };
            default:
                return { border: '#cccccc', background: '#333333' };
        }
    }

    function addLogEntry(message) {
        const entry = document.createElement('div');
        entry.className = 'log-entry';
        entry.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
        logEntriesElement.appendChild(entry);
        logEntriesElement.scrollTop = logEntriesElement.scrollHeight;
    }

    function getNodeColor(status) {
        switch (status) {
            case 'Active':
                return { border: '#00aaff', background: '#2a2a2a' };
            case 'Executing':
                return { border: '#ffff00', background: '#4a4a00' };
            case 'Completed':
                return { border: '#00ff00', background: '#004a00' };
            case 'Failed':
                return { border: '#ff0000', background: '#4a0000' };
            default:
                return { border: '#cccccc', background: '#333333' };
        }
    }
});
