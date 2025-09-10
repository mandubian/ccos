document.addEventListener('DOMContentLoaded', () => {
    const nodes = new vis.DataSet([]);
    const edges = new vis.DataSet([]);

    // Track current graph for multi-graph support
    let currentGraphId = null;

    const container = document.getElementById('graph-visualization');
    console.log('Graph visualization container:', container);
    console.log('Container dimensions:', container ? { width: container.offsetWidth, height: container.offsetHeight } : 'Container not found');

    // Graph History DOM elements - declare early to avoid reference errors
    const graphHistorySelector = document.getElementById('graph-history-selector');
    const loadHistoryGraphBtn = document.getElementById('load-history-graph-btn');
    const deleteHistoryGraphBtn = document.getElementById('delete-history-graph-btn');
    
    const data = { nodes, edges };
    const options = {
        layout: {
            hierarchical: {
                enabled: true,
                sortMethod: 'directed', // Use directed layout for proper parent-child hierarchy
                direction: 'UD', // Up-Down layout (top to bottom = execution order)
                levelSeparation: 200, // Increased separation between levels
                nodeSpacing: 300, // Increased spacing between nodes
                parentCentralization: true, // Center parent nodes above their children
                edgeMinimization: true, // Minimize edge crossings
                blockShifting: true, // Allow block shifting for better layout
                treeSpacing: 200, // Spacing between different branches
            },
        },
        physics: {
            enabled: false, // Disable physics for more predictable execution order layout
            hierarchicalRepulsion: {
                centralGravity: 0.0,
                springLength: 100,
                springConstant: 0.01,
                damping: 0.09,
            },
            solver: 'hierarchicalRepulsion',
        },
        nodes: {
            shape: 'box',
            font: {
                color: '#f0f0f0',
                size: 14,
                face: 'arial',
            },
            color: {
                border: '#00aaff',
                background: '#2a2a2a',
                highlight: {
                    border: '#00ff00',
                    background: '#3a3a3a',
                },
            },
            borderWidth: 2,
            borderWidthSelected: 3,
            shadow: {
                enabled: false,
                color: 'rgba(0,0,0,0.5)',
                size: 5,
                x: 2,
                y: 2,
            },
        },
        edges: {
            arrows: {
                to: {
                    enabled: true,
                    scaleFactor: 0.8,
                    type: 'arrow',
                },
            },
            color: {
            color: '#00aaff',
                highlight: '#00ffaa',
            },
            width: 2,
            shadow: {
                enabled: true,
                color: 'rgba(0,0,0,0.3)',
                size: 3,
                x: 1,
                y: 1,
            },
            smooth: {
                enabled: true,
                type: 'cubicBezier',
                roundness: 0.4,
            },
        },
        interaction: {
            selectConnectedEdges: false,
            hover: true,
            tooltipDelay: 300,
            navigationButtons: true,
            dragNodes: true,
            dragView: true,
            zoomView: true,
            keyboard: {
                enabled: true,
                speed: { x: 10, y: 10, zoom: 0.02 },
                bindToWindow: true,
            },
            multiselect: false,
            hoverConnectedEdges: true,
        },
        configure: {
            enabled: false, // Disable configuration UI for cleaner interface
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
    let selectedIntentId = null;
    let intentNodes = new Map(); // id -> node data
    let intentEdges = new Map(); // id -> edge data
    let generatedPlans = new Map(); // intent_id -> plan data

    // Graph history management
    let graphHistory = new Map(); // graph_id -> {nodes: Map, edges: Map, plans: Map, timestamp: Date, name: String}
    let currentGraphHistory = null; // Store current graph before replacement

    // Local storage management
    const STORAGE_KEY = 'ccos_graph_history';
    const MAX_STORED_GRAPHS = 20; // Limit to prevent storage bloat

    // Functions for local storage persistence
    function saveGraphHistoryToStorage() {
        try {
            // Convert Map to serializable object
            const historyObject = {};
            let savedCount = 0;

            // Sort by timestamp (most recent first) and limit to MAX_STORED_GRAPHS
            const sortedEntries = Array.from(graphHistory.entries())
                .sort((a, b) => b[1].timestamp - a[1].timestamp)
                .slice(0, MAX_STORED_GRAPHS);

            for (const [graphId, graphData] of sortedEntries) {
                historyObject[graphId] = {
                    nodes: Array.from(graphData.nodes.entries()),
                    edges: Array.from(graphData.edges.entries()),
                    plans: Array.from(graphData.plans.entries()),
                    timestamp: graphData.timestamp.toISOString(),
                    rootId: graphData.rootId,
                    name: graphData.name || `Graph ${savedCount + 1}`
                };
                savedCount++;
            }

            const serialized = JSON.stringify(historyObject);
            localStorage.setItem(STORAGE_KEY, serialized);

            console.log(`💾 Saved ${savedCount} graphs to local storage (${serialized.length} bytes)`);
            return true;
        } catch (error) {
            console.error('❌ Failed to save graph history to local storage:', error);
            addLogEntry(`❌ Failed to save graphs to local storage: ${error.message}`);
            return false;
        }
    }

    function loadGraphHistoryFromStorage() {
        try {
            const serialized = localStorage.getItem(STORAGE_KEY);
            if (!serialized) {
                console.log('📭 No saved graphs found in local storage');
                return 0;
            }

            const historyObject = JSON.parse(serialized);
            let loadedCount = 0;

            for (const [graphId, graphData] of Object.entries(historyObject)) {
                // Reconstruct Maps from arrays
                const reconstructedGraph = {
                    nodes: new Map(graphData.nodes),
                    edges: new Map(graphData.edges),
                    plans: new Map(graphData.plans),
                    timestamp: new Date(graphData.timestamp),
                    rootId: graphData.rootId,
                    name: graphData.name || `Graph ${loadedCount + 1}`
                };

                graphHistory.set(graphId, reconstructedGraph);
                loadedCount++;
            }

            console.log(`📖 Loaded ${loadedCount} graphs from local storage`);
            addLogEntry(`📖 Restored ${loadedCount} graphs from previous sessions`);
            
            return loadedCount;
        } catch (error) {
            console.error('❌ Failed to load graph history from local storage:', error);
            addLogEntry(`❌ Failed to load saved graphs: ${error.message}`);
            return 0;
        }
    }

    function clearStoredGraphs() {
        try {
            localStorage.removeItem(STORAGE_KEY);
            console.log('🗑️ Cleared all stored graphs from local storage');
            addLogEntry('🗑️ Cleared all stored graphs from local storage');
            
            // Update the graph history selector
            populateGraphHistorySelector();
            
            return true;
        } catch (error) {
            console.error('❌ Failed to clear stored graphs:', error);
            addLogEntry(`❌ Failed to clear stored graphs: ${error.message}`);
            return false;
        }
    }


    function generateGraphName() {
        // Try to generate a meaningful name from the graph content
        if (intentNodes.size === 0) return `Empty Graph ${graphHistory.size + 1}`;

        // Get the root node
        const rootNode = Array.from(intentNodes.values()).find(node => node.type === 'intent');
        if (rootNode && rootNode.goal) {
            // Truncate long goals for the name
            const shortGoal = rootNode.goal.length > 30
                ? rootNode.goal.substring(0, 30) + '...'
                : rootNode.goal;
            return shortGoal;
        }

        return `Graph ${graphHistory.size + 1} (${intentNodes.size} nodes)`;
    }

    // WebSocket with auto-reconnect and exponential backoff
    let socket = null;
    let reconnectAttempts = 0;
    let connectionStatus = 'disconnected';
    let heartbeatInterval = null;

    function updateConnectionStatus(status, text) {
        connectionStatus = status;
        const statusElement = document.getElementById('connection-status');
        const textElement = document.getElementById('connection-text');

        if (statusElement && textElement) {
            statusElement.className = `status-${status}`;
            textElement.textContent = text;
        }
    }

    function startHeartbeat() {
        if (heartbeatInterval) clearInterval(heartbeatInterval);
        heartbeatInterval = setInterval(() => {
            if (socket && socket.readyState === WebSocket.OPEN) {
                socket.send(JSON.stringify({ type: 'ping' }));
            }
        }, 30000); // Send heartbeat every 30 seconds
    }

    function stopHeartbeat() {
        if (heartbeatInterval) {
            clearInterval(heartbeatInterval);
            heartbeatInterval = null;
        }
    }

    function connect() {
        // Use dynamic host instead of hardcoded localhost
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = window.location.host;
        const wsUrl = `${protocol}//${host}/ws`;

        console.log('Connecting to WebSocket:', wsUrl);
        socket = new WebSocket(wsUrl);

        socket.onopen = () => {
            reconnectAttempts = 0;
            console.log('WebSocket connection established');
            updateConnectionStatus('connected', 'Connected');
            addLogEntry('Connected to CCOS server.');
            updateGoalStatus('Connected to CCOS server.');
            startHeartbeat();
        };

        socket.onmessage = (event) => {
            const message = JSON.parse(event.data);
            console.log('Received WebSocket message:', message);

            // Handle pong responses for heartbeat
            if (message.type === 'pong') {
                console.log('Received heartbeat pong');
                return;
            }

            addLogEntry(`Received event: ${message.type}`);
            handleEvent(message);
        };

        socket.onclose = () => {
            console.log('WebSocket connection closed');
            stopHeartbeat();
            updateConnectionStatus('disconnected', 'Disconnected');
            addLogEntry('Disconnected from CCOS server.');
            updateGoalStatus('Disconnected from CCOS server.');
            scheduleReconnect();
        };

        socket.onerror = (e) => {
            console.error('WebSocket error', e);
            stopHeartbeat();
            updateConnectionStatus('disconnected', 'Connection Error');
            socket.close();
        };
    }

    function scheduleReconnect() {
        reconnectAttempts += 1;
        const delay = Math.min(30000, 1000 * Math.pow(2, reconnectAttempts));
        updateConnectionStatus('connecting', `Reconnecting in ${delay/1000}s...`);
        addLogEntry(`Reconnecting in ${delay/1000}s...`);
        updateGoalStatus(`Reconnecting in ${delay/1000}s...`);
        setTimeout(() => connect(), delay);
    }

    // Initialize connection status
    updateConnectionStatus('connecting', 'Connecting...');

    connect();

    // Cleanup on page unload
    window.addEventListener('beforeunload', () => {
        if (socket) {
            socket.close();
        }
        stopHeartbeat();
    });

    // Load stored graphs from local storage
    const loadedGraphs = loadGraphHistoryFromStorage();

    // Initial status message
    addLogEntry('🚀 CCOS Viewer ready. Generate graphs to explore intent relationships.');
    if (loadedGraphs > 0) {
        addLogEntry(`💾 Restored ${loadedGraphs} graphs from local storage.`);
        addLogEntry('💡 Use listStoredGraphs() to see saved graphs, restoreStoredGraph(id) to load one.');
    } else {
        addLogEntry('💾 Graph history will be automatically saved to local storage.');
    }
    updateGoalStatus('Ready to generate intent graph...');

    // Initialize graph history selector after DOM elements are available
    populateGraphHistorySelector();

    // Real-time input validation
    const goalInput = document.getElementById('goal-input');
    if (goalInput) {
        goalInput.addEventListener('input', (e) => {
            const value = e.target.value.trim();
            const length = value.length;

            // Remove existing validation classes
            goalInput.classList.remove('valid', 'invalid');

            if (length === 0) {
                // Empty input - neutral state
                return;
            }

            if (length < 10) {
                goalInput.classList.add('invalid');
                goalInput.title = 'Goal must be at least 10 characters long';
            } else if (length > 1000) {
                goalInput.classList.add('invalid');
                goalInput.title = 'Goal must be less than 1000 characters';
            } else {
                goalInput.classList.add('valid');
                goalInput.title = 'Goal looks good!';
            }
        });
    }

    // Goal form submission with improved error handling
    const goalForm = document.getElementById('goal-form');
    if (goalForm) {
        goalForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const input = document.getElementById('goal-input');
            const submitBtn = document.getElementById('generate-graph-btn');

            if (!input || !submitBtn) {
                addLogEntry('Error: Form elements not found');
                return;
            }

            const goal = input.value.trim();

            // Input validation
            if (!goal) {
                addLogEntry('Error: Please enter a goal before submitting');
                updateGoalStatus('Please enter a goal');
                input.focus();
                return;
            }

            if (goal.length < 10) {
                addLogEntry('Error: Goal must be at least 10 characters long');
                updateGoalStatus('Goal must be more descriptive');
                input.focus();
                return;
            }

            if (goal.length > 1000) {
                addLogEntry('Error: Goal must be less than 1000 characters');
                updateGoalStatus('Goal is too long, please shorten it');
                return;
            }

            // Disable form during submission
            submitBtn.disabled = true;
            submitBtn.textContent = 'Generating...';
            input.disabled = true;

            addLogEntry(`Generating graph for goal: "${goal}"`);
            updateGoalStatus('Generating intent graph...');

            try {
                const controller = new AbortController();
                const timeoutId = setTimeout(() => controller.abort(), 30000); // 30 second timeout

                const resp = await fetch('/generate-graph', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ goal }),
                    signal: controller.signal
                });

                clearTimeout(timeoutId);

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`✅ Graph generation started: ${result.message || 'Success'}`);
                    updateGoalStatus('Graph generation in progress...');
                } else {
                    let errorMessage = `HTTP ${resp.status}`;
                    try {
                        const errorData = await resp.json();
                        errorMessage = errorData.message || errorData.error || errorMessage;
                    } catch {
                    const text = await resp.text();
                        if (text) errorMessage += `: ${text}`;
                    }

                    addLogEntry(`❌ Failed to generate graph: ${errorMessage}`);
                    updateGoalStatus(`Failed to generate graph: ${errorMessage}`);
                }
            } catch (err) {
                let errorMessage = 'Unknown error occurred';
                if (err.name === 'AbortError') {
                    errorMessage = 'Request timed out (30s)';
                } else if (err.message) {
                    errorMessage = err.message;
                }

                addLogEntry(`❌ Network error: ${errorMessage}`);
                updateGoalStatus(`Network error: ${errorMessage}`);
            } finally {
                // Re-enable form
                submitBtn.disabled = false;
                submitBtn.textContent = 'Generate Graph';
                input.disabled = false;
            }
        });
    }

    // Action buttons
    const generatePlansBtn = document.getElementById('generate-plans-btn');
    const executeBtn = document.getElementById('execute-btn');
    const clearBtn = document.getElementById('clear-btn');

    // Graph control buttons
    const zoomInBtn = document.getElementById('zoom-in-btn');
    const zoomOutBtn = document.getElementById('zoom-out-btn');
    const fitBtn = document.getElementById('fit-btn');
    const togglePhysicsBtn = document.getElementById('toggle-physics-btn');

    // Search functionality
    const nodeSearchInput = document.getElementById('node-search');
    const clearSearchBtn = document.getElementById('clear-search-btn');

    // Save/Load/Export functionality
    const saveGraphBtn = document.getElementById('save-graph-btn');
    const loadGraphBtn = document.getElementById('load-graph-btn');
    const exportGraphBtn = document.getElementById('export-graph-btn');


    // Graph History Selector Functions - define early to avoid reference errors
    function populateGraphHistorySelector() {
        if (!graphHistorySelector) return;
        
        // Clear existing options except the first one
        graphHistorySelector.innerHTML = '<option value="">📚 Select Graph History...</option>';
        
        if (graphHistory.size === 0) {
            const option = document.createElement('option');
            option.value = '';
            option.textContent = 'No graphs in history';
            option.disabled = true;
            graphHistorySelector.appendChild(option);
            return;
        }
        
        // Sort graphs by timestamp (newest first)
        const sortedGraphs = Array.from(graphHistory.entries())
            .sort(([,a], [,b]) => b.timestamp - a.timestamp);
        
        sortedGraphs.forEach(([graphId, graph]) => {
            const option = document.createElement('option');
            option.value = graphId;
            const timeStr = graph.timestamp.toLocaleString();
            const planCount = graph.plans.size;
            option.textContent = `${graph.name} (${timeStr}) - ${planCount} plans`;
            graphHistorySelector.appendChild(option);
        });
        
        console.log(`📚 Populated graph history selector with ${graphHistory.size} graphs`);
    }
    
    function updateGraphHistoryButtons() {
        const selectedGraphId = graphHistorySelector.value;
        const hasSelection = selectedGraphId && selectedGraphId !== '';
        
        if (loadHistoryGraphBtn) {
            loadHistoryGraphBtn.disabled = !hasSelection;
        }
        if (deleteHistoryGraphBtn) {
            deleteHistoryGraphBtn.disabled = !hasSelection;
        }
    }
    
    async function loadSelectedGraphFromHistory() {
        const selectedGraphId = graphHistorySelector.value;
        if (!selectedGraphId || selectedGraphId === '') {
            addLogEntry('❌ No graph selected from history');
            return;
        }
        
        const success = await restoreGraphFromHistory(selectedGraphId);
        if (success) {
            addLogEntry(`✅ Loaded graph "${graphHistory.get(selectedGraphId).name}" from history`);
            updateGoalStatus('Graph loaded from history. Ready to generate plans or execute.');
            
            // Update UI state
            if (generatePlansBtn) generatePlansBtn.disabled = false;
            if (executeBtn) executeBtn.disabled = generatedPlans.size === 0;
            
            // Clear selection
            graphHistorySelector.value = '';
            updateGraphHistoryButtons();
        } else {
            addLogEntry(`❌ Failed to load graph from history`);
        }
    }
    
    function deleteSelectedGraphFromHistory() {
        const selectedGraphId = graphHistorySelector.value;
        if (!selectedGraphId || selectedGraphId === '') {
            addLogEntry('❌ No graph selected from history');
            return;
        }
        
        const graph = graphHistory.get(selectedGraphId);
        if (!graph) {
            addLogEntry('❌ Graph not found in history');
            return;
        }
        
        if (confirm(`Are you sure you want to delete "${graph.name}" from history? This cannot be undone.`)) {
            graphHistory.delete(selectedGraphId);
            saveGraphHistoryToStorage();
            populateGraphHistorySelector();
            updateGraphHistoryButtons();
            addLogEntry(`🗑️ Deleted graph "${graph.name}" from history`);
        }
    }

    if (generatePlansBtn) {
        generatePlansBtn.addEventListener('click', async () => {
            if (!currentGraphId) {
                addLogEntry('❌ Error: No graph available. Generate a graph first.');
                updateGoalStatus('No graph available - generate a graph first');
                return;
            }

            const originalText = generatePlansBtn.textContent;
            generatePlansBtn.disabled = true;
            generatePlansBtn.textContent = 'Generating...';

            addLogEntry('🔄 Generating plans for all intents...');
            updateGoalStatus('Generating plans...');

            try {
                const controller = new AbortController();
                const timeoutId = setTimeout(() => controller.abort(), 60000); // 60 second timeout

                const resp = await fetch('/generate-plans', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ graph_id: currentGraphId }),
                    signal: controller.signal
                });

                clearTimeout(timeoutId);

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`✅ Plan generation started: ${result.message || 'Success'}`);
                    updateGoalStatus('Plan generation in progress...');
                } else {
                    let errorMessage = `HTTP ${resp.status}`;
                    try {
                        const errorData = await resp.json();
                        errorMessage = errorData.message || errorData.error || errorMessage;
                    } catch {
                    const text = await resp.text();
                        if (text) errorMessage += `: ${text}`;
                    }

                    addLogEntry(`❌ Failed to generate plans: ${errorMessage}`);
                    updateGoalStatus(`Failed to generate plans: ${errorMessage}`);
                }
            } catch (err) {
                let errorMessage = 'Unknown error occurred';
                if (err.name === 'AbortError') {
                    errorMessage = 'Request timed out (60s)';
                } else if (err.message) {
                    errorMessage = err.message;
                }

                addLogEntry(`❌ Network error generating plans: ${errorMessage}`);
                updateGoalStatus(`Network error: ${errorMessage}`);
            } finally {
                generatePlansBtn.disabled = false;
                generatePlansBtn.textContent = originalText;
            }
        });
    }

    if (executeBtn) {
        executeBtn.addEventListener('click', async () => {
            if (!currentGraphId) {
                addLogEntry('❌ Error: No graph available. Generate a graph and plans first.');
                updateGoalStatus('No graph available - generate graph and plans first');
                return;
            }

            const originalText = executeBtn.textContent;
            executeBtn.disabled = true;
            executeBtn.textContent = 'Executing...';

            addLogEntry('🚀 Executing plans...');
            updateGoalStatus('Executing plans...');

            try {
                const controller = new AbortController();
                const timeoutId = setTimeout(() => controller.abort(), 120000); // 2 minute timeout

                const resp = await fetch('/execute', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ graph_id: currentGraphId }),
                    signal: controller.signal
                });

                clearTimeout(timeoutId);

                if (resp.ok) {
                    const result = await resp.json();
                    addLogEntry(`✅ Execution started: ${result.message || 'Success'}`);
                    updateGoalStatus('Execution in progress...');
                } else {
                    let errorMessage = `HTTP ${resp.status}`;
                    try {
                        const errorData = await resp.json();
                        errorMessage = errorData.message || errorData.error || errorMessage;
                    } catch {
                    const text = await resp.text();
                        if (text) errorMessage += `: ${text}`;
                    }

                    addLogEntry(`❌ Failed to execute: ${errorMessage}`);
                    updateGoalStatus(`Failed to execute: ${errorMessage}`);
                }
            } catch (err) {
                let errorMessage = 'Unknown error occurred';
                if (err.name === 'AbortError') {
                    errorMessage = 'Request timed out (2min)';
                } else if (err.message) {
                    errorMessage = err.message;
                }

                addLogEntry(`❌ Network error executing plans: ${errorMessage}`);
                updateGoalStatus(`Network error: ${errorMessage}`);
            } finally {
                executeBtn.disabled = false;
                executeBtn.textContent = originalText;
            }
        });
    }

    if (clearBtn) {
        clearBtn.addEventListener('click', () => {
            // Show confirmation if there's data to clear
            const hasCurrentData = nodes.length > 0 || edges.length > 0;
            const hasHistory = graphHistory.size > 0;

            if (hasCurrentData || hasHistory) {
                let message = 'Are you sure you want to clear';
                if (hasCurrentData) message += ' current graph data';
                if (hasCurrentData && hasHistory) message += ' and';
                if (hasHistory) message += ` graph history (${graphHistory.size} graphs)`;
                message += '?';

                const confirmed = confirm(message);
                if (!confirmed) return;
            }

            try {
                // Clear current graph with error handling
                // Clear both local and network DataSets
            nodes.clear();
            edges.clear();
            network.body.data.nodes.clear();
            network.body.data.edges.clear();
            intentNodes.clear();
            intentEdges.clear();
                generatedPlans.clear();

            // Clear UI
                if (rtfsCodeElement) rtfsCodeElement.textContent = '';
                if (selectedIntentInfoElement) selectedIntentInfoElement.textContent = 'Select an intent to view details';
                if (graphStatsElement) graphStatsElement.textContent = 'No graph generated yet';
            updateGoalStatus('Ready to generate intent graph...');

            // Reset state
            currentGraphId = null;
            selectedIntentId = null;

            // Disable buttons
                if (generatePlansBtn) generatePlansBtn.disabled = true;
                if (executeBtn) executeBtn.disabled = true;

                addLogEntry('🧹 Cleared current graph data successfully.');
                if (graphHistory.size > 0) {
                    addLogEntry(`📚 Graph history preserved: ${graphHistory.size} graphs available.`);
                    addLogEntry('💡 Use listStoredGraphs() to see saved graphs from previous sessions.');
                }
            } catch (err) {
                addLogEntry(`❌ Error clearing data: ${err.message}`);
                console.error('Error clearing graph data:', err);
            }
        });
    }

    // Graph control buttons
    if (zoomInBtn) {
        zoomInBtn.addEventListener('click', () => {
            try {
                const currentScale = network.getScale();
                network.moveTo({
                    scale: Math.min(currentScale * 1.2, 2.0), // Max zoom 2x
                    animation: { duration: 300, easingFunction: 'easeInOutQuad' }
                });
                addLogEntry('🔍 Zoomed in');
            } catch (err) {
                addLogEntry(`❌ Error zooming in: ${err.message}`);
            }
        });
    }

    if (zoomOutBtn) {
        zoomOutBtn.addEventListener('click', () => {
            try {
                const currentScale = network.getScale();
                network.moveTo({
                    scale: Math.max(currentScale * 0.8, 0.1), // Min zoom 0.1x
                    animation: { duration: 300, easingFunction: 'easeInOutQuad' }
                });
                addLogEntry('🔍 Zoomed out');
            } catch (err) {
                addLogEntry(`❌ Error zooming out: ${err.message}`);
            }
        });
    }

    if (fitBtn) {
        fitBtn.addEventListener('click', () => {
            try {
                network.fit({
                    animation: { duration: 500, easingFunction: 'easeInOutQuad' }
                });
                addLogEntry('📐 Fit graph to screen');
            } catch (err) {
                addLogEntry(`❌ Error fitting graph: ${err.message}`);
            }
        });
    }

    if (togglePhysicsBtn) {
        let physicsEnabled = true;

        togglePhysicsBtn.addEventListener('click', () => {
            try {
                physicsEnabled = !physicsEnabled;
                network.setOptions({
                    physics: {
                        enabled: physicsEnabled,
                        stabilization: {
                            enabled: physicsEnabled,
                            iterations: 100
                        }
                    }
                });

                togglePhysicsBtn.classList.toggle('active', physicsEnabled);
                togglePhysicsBtn.title = physicsEnabled ? 'Disable Physics' : 'Enable Physics';
                addLogEntry(`⚡ Physics ${physicsEnabled ? 'enabled' : 'disabled'}`);
            } catch (err) {
                addLogEntry(`❌ Error toggling physics: ${err.message}`);
            }
        });

        // Set initial state
        togglePhysicsBtn.classList.add('active');
        togglePhysicsBtn.title = 'Disable Physics';
    }

    // Search functionality
    if (nodeSearchInput && clearSearchBtn) {
        let searchTimeout = null;

        const performSearch = (searchTerm) => {
            try {
                if (!searchTerm.trim()) {
                    // Clear all highlights
                    nodes.forEach(node => {
                        network.body.data.nodes.update({ id: node.id, opacity: 1.0, hidden: false });
                    });
                    edges.forEach(edge => {
                        network.body.data.edges.update({ id: edge.id, opacity: 1.0, hidden: false });
                    });
                    updateGraphStats();
                    return;
                }

                const searchLower = searchTerm.toLowerCase();
                const matchingNodes = new Set();
                const visibleEdges = new Set();

                // Find matching nodes
                nodes.forEach(node => {
                    const nodeData = intentNodes.get(node.id);
                    if (nodeData) {
                        const matches = (
                            (nodeData.label && nodeData.label.toLowerCase().includes(searchLower)) ||
                            (nodeData.id && nodeData.id.toLowerCase().includes(searchLower)) ||
                            (nodeData.goal && nodeData.goal.toLowerCase().includes(searchLower)) ||
                            (nodeData.status && nodeData.status.toLowerCase().includes(searchLower))
                        );

                        if (matches) {
                            matchingNodes.add(node.id);
                        }

                        // Update node visibility and opacity
                        network.body.data.nodes.update({
                            id: node.id,
                            opacity: matches ? 1.0 : 0.3,
                            hidden: false
                        });
                    }
                });

                // Update edge visibility based on connected nodes
                edges.forEach(edge => {
                    const fromVisible = matchingNodes.has(edge.from);
                    const toVisible = matchingNodes.has(edge.to);
                    const edgeVisible = fromVisible || toVisible;

                    if (edgeVisible) {
                        visibleEdges.add(edge.id);
                    }

                    network.body.data.edges.update({
                        id: edge.id,
                        opacity: edgeVisible ? 1.0 : 0.3,
                        hidden: false
                    });
                });

                updateGraphStats();
                addLogEntry(`🔍 Found ${matchingNodes.size} matching nodes`);

                // If only one match, select it
                if (matchingNodes.size === 1) {
                    const nodeId = Array.from(matchingNodes)[0];
                    selectIntent(nodeId);
                    network.selectNodes([nodeId], false);
                }
            } catch (err) {
                addLogEntry(`❌ Error during search: ${err.message}`);
                console.error('Error in search:', err);
            }
        };

        nodeSearchInput.addEventListener('input', (e) => {
            const searchTerm = e.target.value;

            // Clear previous timeout
            if (searchTimeout) {
                clearTimeout(searchTimeout);
            }

            // Debounce search
            searchTimeout = setTimeout(() => {
                performSearch(searchTerm);
            }, 300);
        });

        clearSearchBtn.addEventListener('click', () => {
            nodeSearchInput.value = '';
            performSearch('');
            nodeSearchInput.focus();
        });

        // Clear search on Escape key
        nodeSearchInput.addEventListener('keydown', (e) => {
            if (e.key === 'Escape') {
                nodeSearchInput.value = '';
                performSearch('');
            }
        });
    }

    // Save/Load/Export functionality
    if (saveGraphBtn) {
        saveGraphBtn.addEventListener('click', () => {
            try {
                if (nodes.length === 0) {
                    addLogEntry('❌ No graph to save');
                    return;
                }

                const graphData = {
                    metadata: {
                        savedAt: new Date().toISOString(),
                        graphId: currentGraphId,
                        nodeCount: nodes.length,
                        edgeCount: edges.length,
                        version: '1.0'
                    },
                    nodes: Array.from(intentNodes.values()),
                    edges: Array.from(intentEdges.values()),
                    rtfsCode: rtfsCodeElement ? rtfsCodeElement.textContent : '',
                    goalStatus: goalStatusElement ? goalStatusElement.textContent : ''
                };

                const dataStr = JSON.stringify(graphData, null, 2);
                const blob = new Blob([dataStr], { type: 'application/json' });

                // Create download link
                const url = URL.createObjectURL(blob);
                const link = document.createElement('a');
                link.href = url;
                link.download = `ccos-graph-${currentGraphId || 'unnamed'}-${Date.now()}.json`;
                document.body.appendChild(link);
                link.click();
                document.body.removeChild(link);
                URL.revokeObjectURL(url);

                addLogEntry(`💾 Graph saved successfully (${nodes.length} nodes, ${edges.length} edges)`);
            } catch (err) {
                addLogEntry(`❌ Error saving graph: ${err.message}`);
                console.error('Error saving graph:', err);
            }
        });
    }

    if (loadGraphBtn) {
        loadGraphBtn.addEventListener('click', () => {
            try {
                const input = document.createElement('input');
                input.type = 'file';
                input.accept = '.json';
                input.onchange = async (e) => {
                    const file = e.target.files[0];
                    if (!file) return;

                    const reader = new FileReader();
                    reader.onload = async (event) => {
                        try {
                            const graphData = JSON.parse(event.target.result);

                            // Validate data structure
                            if (!graphData.nodes || !Array.isArray(graphData.nodes)) {
                                throw new Error('Invalid graph file: missing or invalid nodes');
                            }

                            // Clear current graph from both local and network DataSets
                            nodes.clear();
                            edges.clear();
                            network.body.data.nodes.clear();
                            network.body.data.edges.clear();
                            intentNodes.clear();
                            intentEdges.clear();

                            // Load nodes
                            graphData.nodes.forEach(node => {
                                intentNodes.set(node.id, node);
                                const nodeData = {
                                    id: node.id,
                                    label: node.label || node.id,
                                    color: getNodeColor(node.status || 'pending'),
                                    title: `${node.label || node.id}\nStatus: ${node.status || 'pending'}\nType: ${node.type || 'unknown'}`
                                };
                                // Add to both local and network DataSets
                                nodes.add(nodeData);
                                network.body.data.nodes.add(nodeData);
                            });

                            // Load edges
                            if (graphData.edges && Array.isArray(graphData.edges)) {
                                graphData.edges.forEach(edge => {
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
                                    // Add to both local and network DataSets
                                    edges.add(edgeData);
                                    network.body.data.edges.add(edgeData);
                                });
                            }

                            // Load metadata
                            if (graphData.metadata) {
                                currentGraphId = graphData.metadata.graphId;
                                addLogEntry(`📁 Graph loaded: ${graphData.metadata.nodeCount || nodes.length} nodes, ${graphData.metadata.edgeCount || edges.length} edges`);
                            } else {
                                addLogEntry(`📁 Graph loaded: ${nodes.length} nodes, ${edges.length} edges`);
                            }

                            // Load RTFS code if available
                            if (graphData.rtfsCode && rtfsCodeElement) {
                                rtfsCodeElement.textContent = graphData.rtfsCode;
                                if (typeof Prism !== 'undefined') {
                                    Prism.highlightElement(rtfsCodeElement);
                                }
                            }

                            // Update UI
                            updateGraphStats();
                            network.fit({ animation: { duration: 500 } });

                            // Send loaded graph to server to reconstruct CCOS state
                            try {
                                console.log('📤 Sending loaded graph to server...');
                                const loadResp = await fetch('/load-graph', {
                                    method: 'POST',
                                    headers: { 'Content-Type': 'application/json' },
                                    body: JSON.stringify({
                                        nodes: graphData.nodes,
                                        edges: graphData.edges,
                                        root_id: graphData.metadata?.graphId
                                    })
                                });

                                if (loadResp.ok) {
                                    const loadResult = await loadResp.json();
                                    if (loadResult.success && loadResult.graph_id) {
                                        currentGraphId = loadResult.graph_id;
                                        addLogEntry(`✅ Graph loaded and registered with server: ${currentGraphId}`);
                                        console.log('✅ Graph successfully registered with server:', currentGraphId);
                                    } else {
                                        addLogEntry(`⚠️ Graph loaded but server registration failed: ${loadResult.error || 'Unknown error'}`);
                                        console.error('Server registration failed:', loadResult.error);
                                    }
                                } else {
                                    addLogEntry(`❌ Failed to register loaded graph with server`);
                                    console.error('Server registration failed with status:', loadResp.status);
                                }
                            } catch (serverError) {
                                addLogEntry(`❌ Error communicating with server: ${serverError.message}`);
                                console.error('Server communication error:', serverError);
                            }

                            // Enable buttons
                            if (generatePlansBtn) generatePlansBtn.disabled = false;
                            if (executeBtn) executeBtn.disabled = false;

                        } catch (err) {
                            addLogEntry(`❌ Error loading graph: ${err.message}`);
                            console.error('Error loading graph:', err);
                        }
                    };
                    reader.readAsText(file);
                };
                input.click();
            } catch (err) {
                addLogEntry(`❌ Error opening file dialog: ${err.message}`);
                console.error('Error opening file dialog:', err);
            }
        });
    }

    if (exportGraphBtn) {
        exportGraphBtn.addEventListener('click', () => {
            try {
                if (nodes.length === 0) {
                    addLogEntry('❌ No graph to export');
                    return;
                }

                const exportData = {
                    nodes: Array.from(intentNodes.values()),
                    edges: Array.from(intentEdges.values()),
                    metadata: {
                        exportedAt: new Date().toISOString(),
                        nodeCount: nodes.length,
                        edgeCount: edges.length,
                        graphId: currentGraphId
                    }
                };

                // Create multiple export formats
                const formats = {
                    json: () => {
                        const dataStr = JSON.stringify(exportData, null, 2);
                        const blob = new Blob([dataStr], { type: 'application/json' });
                        const url = URL.createObjectURL(blob);
                        const link = document.createElement('a');
                        link.href = url;
                        link.download = `ccos-graph-export-${Date.now()}.json`;
                        link.click();
                        URL.revokeObjectURL(url);
                    },

                    csv: () => {
                        let csvContent = 'Node ID,Label,Status,Goal,Type\n';
                        exportData.nodes.forEach(node => {
                            csvContent += `"${node.id}","${node.label || ''}","${node.status || ''}","${node.goal || ''}","${node.type || ''}"\n`;
                        });
                        csvContent += '\nEdge Source,Edge Target,Edge Type\n';
                        exportData.edges.forEach(edge => {
                            csvContent += `"${edge.source}","${edge.target}","${edge.type || ''}"\n`;
                        });

                        const blob = new Blob([csvContent], { type: 'text/csv' });
                        const url = URL.createObjectURL(blob);
                        const link = document.createElement('a');
                        link.href = url;
                        link.download = `ccos-graph-export-${Date.now()}.csv`;
                        link.click();
                        URL.revokeObjectURL(url);
                    }
                };

                // Show format selection dialog
                const format = prompt('Choose export format (json/csv):', 'json');
                if (format && formats[format.toLowerCase()]) {
                    formats[format.toLowerCase()]();
                    addLogEntry(`📤 Graph exported as ${format.toUpperCase()} (${nodes.length} nodes, ${edges.length} edges)`);
                } else if (format) {
                    addLogEntry('❌ Invalid format. Use "json" or "csv"');
                }

            } catch (err) {
                addLogEntry(`❌ Error exporting graph: ${err.message}`);
                console.error('Error exporting graph:', err);
            }
        });
    }

    // Graph History Controls Event Listeners
    if (graphHistorySelector) {
        graphHistorySelector.addEventListener('change', updateGraphHistoryButtons);
    }

    if (loadHistoryGraphBtn) {
        loadHistoryGraphBtn.addEventListener('click', loadSelectedGraphFromHistory);
    }

    if (deleteHistoryGraphBtn) {
        deleteHistoryGraphBtn.addEventListener('click', deleteSelectedGraphFromHistory);
    }


    // Network click handler for intent selection with error handling
    network.on('selectNode', (params) => {
        try {
            if (params && params.nodes && params.nodes.length > 0) {
            const nodeId = params.nodes[0];
            selectIntent(nodeId);
            }
        } catch (err) {
            addLogEntry(`❌ Error selecting node: ${err.message}`);
            console.error('Error in selectNode handler:', err);
        }
    });

    // Enhanced hover effects
    network.on('hoverNode', (params) => {
        try {
            const nodeId = params.node;
            const node = intentNodes.get(nodeId);
            if (node) {
                // Add hover effect by temporarily increasing border width
                network.body.data.nodes.update({
                    id: nodeId,
                    borderWidth: 4,
                    borderWidthSelected: 5
                });
                console.log(`Hovering over node: ${node.label || nodeId}`);
            }
        } catch (err) {
            console.error('Error in hoverNode handler:', err);
        }
    });

    network.on('blurNode', (params) => {
        try {
            const nodeId = params.node;
            const node = intentNodes.get(nodeId);
            if (node) {
                // Remove hover effect
                network.body.data.nodes.update({
                    id: nodeId,
                    borderWidth: 2,
                    borderWidthSelected: 3
                });
            }
        } catch (err) {
            console.error('Error in blurNode handler:', err);
        }
    });

    // Enhanced double-click handler for zooming to node or showing plan details
    network.on('doubleClick', (params) => {
        try {
            if (params.nodes && params.nodes.length > 0) {
                const nodeId = params.nodes[0];
                const node = nodes.get(nodeId);

                // If node has a plan, show plan details in RTFS container
                if (node && node.has_plan) {
                    showPlanDetails(node);
                    addLogEntry(`📋 Showing plan in RTFS container: ${node.original_label || node.label}`);
                } else {
                    // Default zoom behavior
                    const nodePosition = network.getPositions([nodeId])[nodeId];
                    network.moveTo({
                        position: nodePosition,
                        scale: 1.5,
                        animation: { duration: 500, easingFunction: 'easeInOutQuad' }
                    });

                    selectIntent(nodeId);
                    addLogEntry(`🎯 Zoomed to node: ${intentNodes.get(nodeId)?.label || nodeId}`);
                }
            }
        } catch (err) {
            addLogEntry(`❌ Error in double-click handler: ${err.message}`);
            console.error('Error in doubleClick handler:', err);
        }
    });

    // Add single-click handler for plan details in RTFS container
    network.on('click', (params) => {
        try {
            if (params.nodes && params.nodes.length > 0) {
                const nodeId = params.nodes[0];
                const node = nodes.get(nodeId);

                // If node has a plan, show plan details in RTFS container on single click
                if (node && node.has_plan) {
                    showPlanDetails(node);
                } else {
                    // Reset RTFS container if clicking on a node without a plan
                    hidePlanDetails();
                    // Default selection behavior
                    selectIntent(nodeId);
                }
            } else {
                // Reset RTFS container if clicking on empty space
                hidePlanDetails();
            }
        } catch (err) {
            console.error('Error in click handler:', err);
        }
    });

    // Handle network errors and warnings
    network.on('configChange', (config) => {
        console.log('Network configuration changed:', config);
    });

    network.on('stabilized', (iterations) => {
        console.log(`Network stabilized after ${iterations} iterations`);
    });

    // Add global error handling
    window.addEventListener('error', (event) => {
        addLogEntry(`❌ JavaScript error: ${event.error?.message || 'Unknown error'}`);
        console.error('Global JavaScript error:', event.error);
    });

    window.addEventListener('unhandledrejection', (event) => {
        addLogEntry(`❌ Unhandled promise rejection: ${event.reason?.message || event.reason}`);
        console.error('Unhandled promise rejection:', event.reason);
    });

    function selectIntent(intentId) {
        try {
        selectedIntentId = intentId;
        const node = intentNodes.get(intentId);
            if (node && selectedIntentInfoElement) {
                const status = node.status || 'pending';
                const statusClass = `status-${status.toLowerCase()}`;
            selectedIntentInfoElement.innerHTML = `
                    <strong>ID:</strong> ${node.id || 'N/A'}<br>
                <strong>Name:</strong> ${node.label || 'N/A'}<br>
                    <strong>Status:</strong> <span class="${statusClass}">${status}</span><br>
                <strong>Goal:</strong> ${node.goal || 'N/A'}<br>
                    <strong>Created:</strong> ${node.created_at ? new Date(node.created_at * 1000).toLocaleString() : 'N/A'}<br>
                    <strong>Type:</strong> ${node.type || 'N/A'}
                `;
                addLogEntry(`📋 Selected intent: ${node.label || node.id}`);
            } else {
                if (selectedIntentInfoElement) {
                    selectedIntentInfoElement.textContent = 'Intent not found in graph data';
                }
                addLogEntry(`⚠️ Intent ${intentId} not found in current graph data`);
            }
        } catch (err) {
            addLogEntry(`❌ Error displaying intent details: ${err.message}`);
            console.error('Error in selectIntent:', err);
            if (selectedIntentInfoElement) {
                selectedIntentInfoElement.textContent = 'Error loading intent details';
            }
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
                console.log('📡 RECEIVED GraphGenerated event:', event.data);
                console.log('🔍 WebSocket message sequence check - timestamp:', Date.now());
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
                network.body.data.nodes.update(n);
            });
        }
        if (Array.isArray(data.edges)) {
            data.edges.forEach(e => {
                const edgeId = `${e.from}--${e.to}`;
                intentEdges.set(edgeId, e);
                network.body.data.edges.update({ ...e, id: edgeId });
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
        console.log('🔄 Node status change:', data);

        const currentNode = nodes.get(data.id);
        if (currentNode) {
            const oldStatus = currentNode.status;
            const nodeUpdate = { id: data.id };

            // Handle different status changes
            if (data.status === 'has_plan') {
                // Add plan indicator to the node
                nodeUpdate.has_plan = true;
                nodeUpdate.plan_id = data.details?.plan_id;
                nodeUpdate.plan_body_preview = data.details?.plan_body_preview;

                // Update the label to show plan indicator
                let newLabel = currentNode.original_label || currentNode.label;
                if (newLabel && !newLabel.includes('📋')) {
                    newLabel = newLabel + ' 📋';
                }
                nodeUpdate.label = newLabel;
                nodeUpdate.original_label = currentNode.original_label || currentNode.label;

                // Change border color to indicate plan availability
                nodeUpdate.color = {
                    border: '#00ff88',
                    background: currentNode.color?.background || '#2a2a2a',
                    highlight: { border: '#88ffaa', background: '#3a3a3a' }
                };

                nodeUpdate.title = `${currentNode.original_label || currentNode.label}\n📋 Has Plan Available\nClick to view plan details`;

                addLogEntry(`📋 Plan generated for intent: ${currentNode.original_label || currentNode.label}`);
            } else {
                // Handle other status changes
                nodeUpdate.status = data.status;
                nodeUpdate.color = getNodeColor(data.status);
                nodeUpdate.title = `${currentNode.label}\nStatus: ${data.status}`;
            }

            // Add animation effect for status changes
            if (oldStatus !== data.status) {
                nodeUpdate.shadow = { enabled: true, color: 'rgba(255, 255, 0, 0.5)', size: 15, x: 0, y: 0 };
                setTimeout(() => {
                    const resetUpdate = { id: data.id, shadow: false };
                    network.body.data.nodes.update(resetUpdate);
                }, 1500);
            }

            network.body.data.nodes.update(nodeUpdate);

            // Update selected intent info if this node is selected
            if (selectedIntentId === data.id) {
                selectIntent(data.id);
            }

            addLogEntry(`Intent ${data.id} status changed: ${oldStatus || 'unknown'} → ${data.status}`);
        }
    }

    function handleStepLog(data) {
        addLogEntry(`[${data.step}] ${data.status}: ${data.message}`);
        if (data.details) {
            addLogEntry(`Details: ${JSON.stringify(data.details, null, 2)}`);
        }
    }

    function storeCurrentGraphInHistory(graphName = null) {
        if (currentGraphId && (intentNodes.size > 0 || intentEdges.size > 0)) {
            // Generate a human-readable name if not provided
            const name = graphName || generateGraphName();

            // Store current graph state in history
            console.log('💾 Storing graph in history:');
            console.log('  - Graph ID:', currentGraphId);
            console.log('  - Nodes to save:', intentNodes.size);
            console.log('  - Edges to save:', intentEdges.size);
            console.log('  - Plans to save:', generatedPlans.size);
            console.log('  - Edge details:', Array.from(intentEdges.entries()));
            
            graphHistory.set(currentGraphId, {
                nodes: new Map(intentNodes),
                edges: new Map(intentEdges),
                plans: new Map(generatedPlans),
                timestamp: new Date(),
                rootId: currentGraphId,
                name: name
            });

            console.log(`📚 Stored graph "${name}" (${currentGraphId}) in history`);

            // Auto-save to local storage
            saveGraphHistoryToStorage();
            
            // Update the graph history selector
            populateGraphHistorySelector();
        }
    }

    // Function to restore a graph from history
    function restoreGraphFromHistory(graphId) {
        const historicalGraph = graphHistory.get(graphId);
        if (!historicalGraph) {
            console.error(`❌ Graph ${graphId} not found in history`);
            return false;
        }

        // Store current graph before switching
        storeCurrentGraphInHistory();

        // Clear current state from both local and network DataSets
        console.log('🧹 Clearing existing graph data before restoration...');
        console.log('📊 Before clearing - local nodes:', nodes.length, 'local edges:', edges.length);
        console.log('📊 Before clearing - network nodes:', network.body.data.nodes.length, 'network edges:', network.body.data.edges.length);
        
        // Get all existing items before clearing for debugging
        const existingNodes = nodes.get();
        const existingEdges = edges.get();
        const existingNetworkNodes = network.body.data.nodes.get();
        const existingNetworkEdges = network.body.data.edges.get();
        
        console.log('📋 Existing local nodes:', existingNodes.map(n => n.id));
        console.log('📋 Existing local edges:', existingEdges.map(e => e.id));
        console.log('📋 Existing network nodes:', existingNetworkNodes.map(n => n.id));
        console.log('📋 Existing network edges:', existingNetworkEdges.map(e => e.id));
        
        // Clear all DataSets
        nodes.clear();
        edges.clear();
        network.body.data.nodes.clear();
        network.body.data.edges.clear();
        intentNodes.clear();
        intentEdges.clear();
        generatedPlans.clear();
        
        console.log('📊 After clearing - local nodes:', nodes.length, 'local edges:', edges.length);
        console.log('📊 After clearing - network nodes:', network.body.data.nodes.length, 'network edges:', network.body.data.edges.length);
        
        // Force clear any remaining items
        const remainingLocalNodes = nodes.get();
        const remainingLocalEdges = edges.get();
        const remainingNetworkNodes = network.body.data.nodes.get();
        const remainingNetworkEdges = network.body.data.edges.get();
        
        if (remainingLocalNodes.length > 0 || remainingLocalEdges.length > 0 || 
            remainingNetworkNodes.length > 0 || remainingNetworkEdges.length > 0) {
            console.error('❌ DATASETS NOT CLEARED PROPERLY!');
            console.error('Remaining local nodes:', remainingLocalNodes.length, 'Remaining local edges:', remainingLocalEdges.length);
            console.error('Remaining network nodes:', remainingNetworkNodes.length, 'Remaining network edges:', remainingNetworkEdges.length);
            
            // Force clear by removing all items individually
            remainingLocalNodes.forEach(node => {
                console.log(`🗑️ Force removing local node: ${node.id}`);
                nodes.remove(node.id);
            });
            remainingLocalEdges.forEach(edge => {
                console.log(`🗑️ Force removing local edge: ${edge.id}`);
                edges.remove(edge.id);
            });
            remainingNetworkNodes.forEach(node => {
                console.log(`🗑️ Force removing network node: ${node.id}`);
                network.body.data.nodes.remove(node.id);
            });
            remainingNetworkEdges.forEach(edge => {
                console.log(`🗑️ Force removing network edge: ${edge.id}`);
                network.body.data.edges.remove(edge.id);
            });
            
            console.log('📊 After force clear - local nodes:', nodes.length, 'local edges:', edges.length);
            console.log('📊 After force clear - network nodes:', network.body.data.nodes.length, 'network edges:', network.body.data.edges.length);
        }
        
        // Force network redraw after clearing
        network.redraw();
        console.log('✅ Graph clearing completed');

        // Restore from history
        currentGraphId = historicalGraph.rootId;
        intentNodes = new Map(historicalGraph.nodes);
        intentEdges = new Map(historicalGraph.edges);
        generatedPlans = new Map(historicalGraph.plans);
        
        console.log('📊 Restoring graph data:');
        console.log('  - Nodes:', intentNodes.size);
        console.log('  - Edges:', intentEdges.size);
        console.log('  - Plans:', generatedPlans.size);
        console.log('  - Root ID:', currentGraphId);
        console.log('  - Historical edges:', Array.from(intentEdges.entries()));

        // Inform server to rehydrate this graph into CCOS so that plan generation works
        (async () => {
            try {
                const rehydrateResp = await fetch('/load-graph', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        nodes: Array.from(intentNodes.values()),
                        edges: Array.from(intentEdges.values()),
                        root_id: currentGraphId
                    })
                });
                if (rehydrateResp.ok) {
                    const r = await rehydrateResp.json();
                    if (r.success) {
                        console.log(`✅ Server rehydrated graph ${r.graph_id} into CCOS`);
                    } else {
                        console.warn('⚠️ Server failed to rehydrate graph:', r.error);
                    }
                } else {
                    console.warn('⚠️ /load-graph returned non-OK');
                }
            } catch (e) {
                console.warn('⚠️ Error rehydrating graph on server:', e);
            }
        })();

        // Calculate depth-based levels for proper tree visualization
        const nodeDepths = new Map();
        const rootNode = Array.from(intentNodes.values()).find(node => node.is_root);
        
        if (rootNode) {
            // BFS to calculate depths from root
            const queue = [{ nodeId: rootNode.id, depth: 0 }];
            const visited = new Set();
            
            while (queue.length > 0) {
                const { nodeId, depth } = queue.shift();
                if (visited.has(nodeId)) continue;
                
                visited.add(nodeId);
                nodeDepths.set(nodeId, depth);
                
                // Find children of this node
                const children = Array.from(intentEdges.values())
                    .filter(edge => edge.source === nodeId)
                    .map(edge => edge.target);
                
                children.forEach(childId => {
                    if (!visited.has(childId)) {
                        queue.push({ nodeId: childId, depth: depth + 1 });
                    }
                });
            }
        }

        console.log(`📋 Restored graph with depth-based levels (${intentNodes.size} nodes)`);
        console.log('🌳 Node depths calculated:', Object.fromEntries(nodeDepths));

        // Add a small delay to ensure clearing is complete before adding new nodes
        setTimeout(async () => {
            console.log('🔄 Starting to add restored nodes after clearing delay...');
            
            // Rebuild vis.js data
            intentNodes.forEach((node, nodeId) => {
            const isRoot = node.is_root === true;
            const baseTitle = `${node.label || nodeId}\nStatus: ${node.status || 'pending'}\nType: ${node.type || 'unknown'}`;

            let nodeData;
            if (isRoot) {
                // Root node: special styling, positioned at top level
                nodeData = {
                    id: nodeId,
                    label: node.label || nodeId,
                    level: 0, // Force root node to be at the top level
                    color: {
                        border: '#FFD700', // Gold border for root
                        background: '#2a2a2a',
                        highlight: { border: '#FFD700', background: '#3a3a3a' }
                    },
                    title: `${baseTitle}\n\n🎯 Root Intent - Orchestrates execution of child intents`,
                    shape: 'diamond',
                    size: 30,
                    font: { size: 16, color: '#FFD700', face: 'arial' }
                };
            } else {
                // Child nodes: depth-based level with execution order in label
                const depth = nodeDepths.get(nodeId) || 1;
                nodeData = {
                    id: nodeId,
                    label: node.label || nodeId,
                    level: depth, // Use depth-based level for proper tree visualization
                    color: getNodeColor(node.status || 'pending'),
                    title: `${baseTitle}\nExecution Order: ${node.execution_order || 'N/A'}\nDepth Level: ${depth}\n\n💡 Same depth = same execution level, numbers show sequence`
                };
            }

            // Check if node already exists before adding - check both DataSets
            const existingLocalNode = nodes.get(nodeId);
            const existingNetworkNode = network.body.data.nodes.get(nodeId);
            
            console.log(`🔍 Checking node ${nodeId} for duplicates...`);
            console.log(`📊 Local node exists:`, !!existingLocalNode);
            console.log(`📊 Network node exists:`, !!existingNetworkNode);
            console.log(`📊 Current nodes count - local: ${nodes.length}, network: ${network.body.data.nodes.length}`);

            if (existingLocalNode || existingNetworkNode) {
                console.warn(`⚠️ NODE ALREADY EXISTS! ID: ${nodeId} - updating instead`);
                console.warn('Existing local node:', existingLocalNode);
                console.warn('Existing network node:', existingNetworkNode);
                console.warn('Attempting to add:', nodeData);

                // Update existing node instead of adding (nodes and network.body.data.nodes are the same object)
                try {
                    nodes.update(nodeData);
                    console.log(`🔄 Updated existing node: ${node.label || nodeId} (ID: ${nodeId})`);
                } catch (updateError) {
                    console.error(`❌ Failed to update node ${nodeId}:`, updateError);
                    console.error('Node data:', nodeData);
                    return; // Skip this node
                }
            } else {
                // Double-check by trying to get the node again right before adding
                const finalCheckLocal = nodes.get(nodeId);
                const finalCheckNetwork = network.body.data.nodes.get(nodeId);
                
                if (finalCheckLocal || finalCheckNetwork) {
                    console.warn(`⚠️ NODE FOUND IN FINAL CHECK! ID: ${nodeId} - skipping addition`);
                    console.warn('Final check local:', finalCheckLocal);
                    console.warn('Final check network:', finalCheckNetwork);
                    return; // Skip this node
                }
                
                // Add new node (nodes and network.body.data.nodes are the same object)
                try {
                    console.log(`➕ Attempting to add node: ${nodeId}`);
                    nodes.add(nodeData);
                    console.log(`✅ Added node: ${node.label || nodeId} (ID: ${nodeId})`);
                } catch (error) {
                    console.error(`❌ Failed to add node ${nodeId}:`, error);
                    console.error('Node data:', nodeData);
                    console.error('Current nodes in DataSet:', nodes.get().map(n => n.id));
                }
            }
            });

            // Add edges after nodes are added
            console.log('🔄 Adding restored edges...');
            console.log('📊 Total edges to add:', intentEdges.size);
            intentEdges.forEach((edge, edgeId) => {
                console.log(`🔗 Processing edge ${edgeId}:`, edge);
            const edgeData = {
                id: edgeId,
                from: edge.source,
                to: edge.target,
                label: edge.type || '',
                arrows: 'to',
                color: '#00aaff'
            };
            // Check if edge already exists before adding
            const existingLocalEdge = edges.get(edgeId);
            const existingNetworkEdge = network.body.data.edges.get(edgeId);

            if (existingLocalEdge || existingNetworkEdge) {
                console.warn(`⚠️ EDGE ALREADY EXISTS! ID: ${edgeId} - skipping`);
                console.warn('Existing local edge:', existingLocalEdge);
                console.warn('Existing network edge:', existingNetworkEdge);
                console.warn('Attempting to add:', edgeData);
                return; // Skip this edge
            }

            // Add new edge (edges and network.body.data.edges are the same object)
            try {
                edges.add(edgeData);
                console.log(`✅ Added edge: ${edgeId}`);
            } catch (error) {
                console.error(`❌ Failed to add edge ${edgeId}:`, error);
                console.error('Edge data:', edgeData);
            }
            });

            // Force network redraw to ensure edges are visible
            network.redraw();

            // Reset selection and update UI
            selectedIntentId = null;
            updateGraphStats();
            updateGoalStatus(`Restored graph: ${currentGraphId}`);
            addLogEntry(`📚 Restored graph from history: ${currentGraphId} (${intentNodes.size} nodes, ${intentEdges.size} edges)`);

            // Update button states
            if (generatePlansBtn) generatePlansBtn.disabled = false;
            if (executeBtn) executeBtn.disabled = generatedPlans.size === 0;

            // Fit and redraw
            network.redraw();
            network.fit();
            
            console.log('✅ Graph restoration completed');
            
            // Try to retrieve plans from server for this graph
            try {
                console.log('🔍 Checking for existing plans on server...');
                const plansResponse = await fetch('/get-plans', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ graph_id: currentGraphId })
                });
                
                if (plansResponse.ok) {
                    const plansResult = await plansResponse.json();
                    if (plansResult.success && plansResult.plans && plansResult.plans.length > 0) {
                        console.log(`📋 Retrieved ${plansResult.plans.length} plans from server`);
                        
                        // Store the retrieved plans
                        for (const plan of plansResult.plans) {
                            generatedPlans.set(plan.intent_id, {
                                plan_id: plan.plan_id,
                                rtfs_code: plan.body,
                                intent_id: plan.intent_id,
                                status: plan.status
                            });
                        }
                        
                        // Update UI to show plan indicators on nodes
                        console.log('🔄 Updating node UI to show plan indicators...');
                        for (const plan of plansResult.plans) {
                            const nodeId = plan.intent_id;
                            const node = nodes.get(nodeId);
                            
                            if (node) {
                                console.log(`📋 Updating node ${nodeId} to show plan indicator`);
                                
                                // Update the node to show it has a plan
                                const nodeUpdate = {
                                    id: nodeId,
                                    has_plan: true,
                                    plan_id: plan.plan_id
                                };
                                
                                // Add plan indicator to label if not already present
                                let newLabel = node.original_label || node.label;
                                if (newLabel && !newLabel.includes('📋')) {
                                    newLabel = newLabel + ' 📋';
                                    nodeUpdate.label = newLabel;
                                    nodeUpdate.original_label = node.original_label || node.label;
                                }
                                
                                // Change border color to indicate plan availability
                                nodeUpdate.color = {
                                    border: '#00ff88',
                                    background: node.color?.background || '#2a2a2a',
                                    highlight: { border: '#88ffaa', background: '#3a3a3a' }
                                };
                                
                                nodeUpdate.title = `${node.original_label || node.label}\n📋 Has Plan Available\nClick to view plan details`;
                                
                                // Update the node in the network
                                try {
                                    nodes.update(nodeUpdate);
                                    console.log(`✅ Updated node ${nodeId} with plan indicator`);
                                } catch (error) {
                                    console.error(`❌ Failed to update node ${nodeId}:`, error);
                                }
                            } else {
                                console.warn(`⚠️ Node ${nodeId} not found in network for plan update`);
                            }
                        }
                        
                        // Update button states
                        if (executeBtn) executeBtn.disabled = generatedPlans.size === 0;
                        
                        addLogEntry(`📋 Retrieved ${plansResult.plans.length} plans from server`);
                    } else {
                        console.log('📋 No plans found on server for this graph');
                        addLogEntry('📋 No plans found on server. Use "Generate Plans" button to create them.');
                    }
                } else {
                    console.log('⚠️ Failed to retrieve plans from server');
                    addLogEntry('⚠️ Could not retrieve plans from server');
                }
            } catch (error) {
                console.error('Error retrieving plans from server:', error);
                addLogEntry(`⚠️ Error retrieving plans: ${error.message}`);
            }
        }, 100); // 100ms delay to ensure clearing is complete

        return true;
    }

    // Function to list available graphs in history
    function listGraphHistory() {
        console.log('📚 In-Memory Graph History:');
        if (graphHistory.size === 0) {
            console.log('  No graphs in history');
            return;
        }

        graphHistory.forEach((graph, graphId) => {
            const time = graph.timestamp.toLocaleTimeString();
            const planCount = graph.plans.size;
            console.log(`  📊 ${graph.name} (${graphId}) - ${graph.nodes.size} nodes, ${graph.edges.size} edges, ${planCount} plans (${time})`);
        });

        console.log(`\n💡 To restore a graph, use: restoreGraphFromHistory('${Array.from(graphHistory.keys())[0]}')`);
    }

    // Function to list stored graphs from localStorage
    function listStoredGraphs() {
        console.log('💾 Stored Graphs (localStorage):');
        if (graphHistory.size === 0) {
            console.log('  No graphs stored');
            return;
        }

        let index = 1;
        graphHistory.forEach((graph, graphId) => {
            const time = graph.timestamp.toLocaleString();
            const planCount = graph.plans.size;
            console.log(`  ${index}. 📊 ${graph.name}`);
            console.log(`     ID: ${graphId}`);
            console.log(`     Nodes: ${graph.nodes.size}, Edges: ${graph.edges.size}, Plans: ${planCount}`);
            console.log(`     Saved: ${time}`);
            console.log(`     Restore: restoreStoredGraph("${graphId}")`);
            console.log('');
            index++;
        });
    }

    // Function to restore a stored graph from localStorage
    async function restoreStoredGraph(graphId) {
        return await restoreGraphFromHistory(graphId);
    }

    // Enhanced clear function to optionally clear stored graphs
    function clearAllStoredGraphs() {
        if (confirm('Are you sure you want to permanently delete all stored graphs from local storage? This cannot be undone.')) {
            clearStoredGraphs();
            graphHistory.clear();
            addLogEntry('🗑️ All stored graphs cleared from memory and local storage');
        }
    }


    // Expose functions globally for debugging/console access
    window.restoreGraphFromHistory = restoreGraphFromHistory;
    window.listGraphHistory = listGraphHistory;
    window.restoreStoredGraph = restoreStoredGraph;
    window.listStoredGraphs = listStoredGraphs;
    window.clearStoredGraphs = clearStoredGraphs;
    window.clearAllStoredGraphs = clearAllStoredGraphs;
    window.graphHistory = graphHistory;

    // Add console help message
    console.log('🔧 CCOS Graph Management Commands:');
    console.log('  listGraphHistory() - Show in-memory graphs from current session');
    console.log('  listStoredGraphs() - Show all graphs saved in localStorage');
    console.log('  restoreGraphFromHistory("graph-id") - Restore from memory');
    console.log('  restoreStoredGraph("graph-id") - Restore from localStorage');
    console.log('  clearStoredGraphs() - Clear all graphs from localStorage');
    console.log('  clearAllStoredGraphs() - Clear everything (with confirmation)');
    console.log('📚 Graphs persist in localStorage across browser sessions!');

    let lastProcessedGraphId = null;

    function handleGraphGenerated(data) {
        console.log('handleGraphGenerated called with data:', data);
        console.log('🔍 CALL STACK - handleGraphGenerated called at:', new Error().stack);

        // Check if we've already processed this graph
        if (lastProcessedGraphId === data.graph_id) {
            console.warn('⚠️ DUPLICATE GRAPH PROCESSING DETECTED! Skipping duplicate call for:', data.graph_id);
            return;
        }
        lastProcessedGraphId = data.graph_id;

        // Set current graph ID for multi-graph support
        currentGraphId = data.graph_id;
        console.log('📊 Current graph ID set to:', currentGraphId);

        // Note: We'll store the new graph in history after it's fully processed

        // Each new graph generation replaces the current view (Option 1: Simple Graph Replacement)
        // Since each graph has a unique graph ID, we can safely clear and replace
        console.log('🔄 Replacing current graph with new one (graph ID:', data.graph_id, ')');

        // Clear existing graph data - always do this for new graph generations
        // Clear the shared DataSets (local and network DataSets are the same objects)
        console.log('🧹 Clearing shared DataSets...');
        console.log('📊 Before clearing - nodes:', nodes.length, 'edges:', edges.length);

        // Clear the DataSets (they're all the same objects)
        nodes.clear();
        edges.clear();

        console.log('📊 After clearing - nodes:', nodes.length, 'edges:', edges.length);

        // Double-check that DataSets are actually empty
        if (edges.length > 0) {
            console.error('❌ EDGES DATASET NOT CLEARED PROPERLY!');
            console.error('Remaining edges:', edges.get());
            // Force clear by removing all items
            edges.get().forEach(edge => {
                edges.remove(edge.id);
            });
            console.log('📊 Edges after force clear:', edges.length);
        }

        // Force network redraw after clearing
        console.log('🔄 Forcing network redraw after clearing...');
        network.redraw();

        intentNodes.clear();
        intentEdges.clear();
        generatedPlans.clear(); // Also clear plans since they're specific to this graph

        console.log('📊 After clearing - Local nodes:', nodes.length, 'Network nodes:', network.body.data.nodes.length);
        console.log('✅ All graph data cleared successfully');

        // Update current graph ID
        currentGraphId = data.root_id;

        // Reset selection state
        selectedIntentId = null;

        // Update UI
        generatePlansBtn.disabled = false;
        updateGoalStatus('Graph generated successfully. Ready to generate plans.');
        addLogEntry(`📊 New graph generated with root ID: ${data.root_id}`);

        // Disable buttons that require the graph
        if (generatePlansBtn) generatePlansBtn.disabled = false;
        if (executeBtn) executeBtn.disabled = true; // Plans not generated yet

        // Track changes for better user feedback
        let nodesAdded = 0;

        // Calculate depth-based levels for proper tree visualization
        const nodeDepths = new Map();
        const rootNode = data.nodes.find(node => node.is_root);
        
        if (rootNode) {
            // BFS to calculate depths from root
            const queue = [{ nodeId: rootNode.id, depth: 0 }];
            const visited = new Set();
            
            while (queue.length > 0) {
                const { nodeId, depth } = queue.shift();
                if (visited.has(nodeId)) continue;
                
                visited.add(nodeId);
                nodeDepths.set(nodeId, depth);
                
                // Find children of this node
                const children = data.edges
                    .filter(edge => edge.source === nodeId)
                    .map(edge => edge.target);
                
                children.forEach(childId => {
                    if (!visited.has(childId)) {
                        queue.push({ nodeId: childId, depth: depth + 1 });
                    }
                });
            }
        }

        // Add nodes to the graph (they come pre-sorted from server with execution order)
        if (Array.isArray(data.nodes)) {
            console.log('📊 Processing nodes with depth-based levels:', data.nodes.length, 'nodes');
            console.log('🔧 Adding nodes to both local and network DataSets...');
            console.log('📋 Node data received:', data.nodes);
            console.log('🌳 Node depths calculated:', Object.fromEntries(nodeDepths));

            // Check for duplicate IDs before processing
            const nodeIds = data.nodes.map(n => n.id);
            const uniqueIds = new Set(nodeIds);
            if (nodeIds.length !== uniqueIds.size) {
                console.error('❌ DUPLICATE NODE IDs DETECTED IN SERVER RESPONSE!');
                console.error('All node IDs:', nodeIds);
                console.error('Unique IDs:', Array.from(uniqueIds));
                // Find duplicates
                const duplicates = nodeIds.filter((id, index) => nodeIds.indexOf(id) !== index);
                console.error('Duplicate IDs:', [...new Set(duplicates)]);
                console.error('Full node data:', data.nodes);
            }

            data.nodes.forEach(node => {
                // Handle root node specially
                const isRoot = node.is_root === true;
                const baseTitle = `${node.label || node.id}\nStatus: ${node.status || 'pending'}\nType: ${node.type || 'unknown'}`;

                let nodeData;
                if (isRoot) {
                    // Root node: special styling, positioned at top level
                    nodeData = {
                    id: node.id,
                    label: node.label || node.id,
                        level: 0, // Force root node to be at the top level
                        color: {
                            border: '#FFD700', // Gold border for root
                            background: '#2a2a2a',
                            highlight: { border: '#FFD700', background: '#3a3a3a' }
                        },
                        title: `${baseTitle}\n\n🎯 Root Intent - Orchestrates execution of child intents`,
                        shape: 'diamond',
                        size: 30,
                        font: { size: 16, color: '#FFD700', face: 'arial' }
                    };
                } else {
                    // Child nodes: depth-based level with execution order in label
                    const depth = nodeDepths.get(node.id) || 1;
                    nodeData = {
                        id: node.id,
                        label: node.label || node.id,
                        level: depth, // Use depth-based level for proper tree visualization
                    color: getNodeColor(node.status || 'pending'),
                        title: `${baseTitle}\nExecution Order: ${node.execution_order || 'N/A'}\nDepth Level: ${depth}\n\n💡 Same depth = same execution level, numbers show sequence`
                    };

                    // Add visual emphasis for execution order
                    if (node.execution_order) {
                        const baseLabel = node.label || node.id;
                        nodeData.label = `🔢 ${node.execution_order}. ${baseLabel.replace(/^\d+\.\s*/, '')}`;
                    }
                }

                // Add execution order as a custom property for potential styling
                if (node.execution_order && node.execution_order !== null) {
                    nodeData.execution_order = node.execution_order;
                }

                // Add root flag for potential styling
                if (isRoot) {
                    nodeData.is_root = true;
                }

                // Check if node already exists before adding
                const existingLocalNode = nodes.get(node.id);
                const existingNetworkNode = network.body.data.nodes.get(node.id);

                if (existingLocalNode || existingNetworkNode) {
                    console.warn(`⚠️ NODE ALREADY EXISTS! ID: ${node.id} - updating instead`);
                    console.warn('Existing local node:', existingLocalNode);
                    console.warn('Existing network node:', existingNetworkNode);
                    console.warn('Attempting to add:', node);

                    // Update existing node instead of adding
                    try {
                        nodes.update(nodeData);
                        network.body.data.nodes.update(nodeData);
                        intentNodes.set(node.id, node);
                        nodesAdded++;
                        console.log(`🔄 Updated existing node: ${node.label || node.id} (ID: ${node.id})`);
                    } catch (updateError) {
                        console.error(`❌ Failed to update node ${node.id}:`, updateError);
                        console.error('Node data:', nodeData);
                        return; // Skip this node
                    }
                    return; // Skip the add operation below
                }

                // Add new node to shared DataSet (network.body.data.nodes and nodes are the same object)
                try {
                    network.body.data.nodes.add(nodeData);
                    intentNodes.set(node.id, node);
                    nodesAdded++;
                    if (isRoot) {
                        console.log(`👑 Added root node: ${node.label || node.id} (ID: ${node.id})`);
                    } else {
                        console.log(`Added node ${node.execution_order || '?'}: ${node.label || node.id} (ID: ${node.id})`);
                    }
                } catch (error) {
                    console.error(`❌ Failed to add node ${node.id}:`, error);
                    console.error('Node data:', nodeData);
                }
            });
        }

        // Add edges to the graph (with delay to ensure clearing is complete)
        if (Array.isArray(data.edges)) {
            console.log('Processing edges:', data.edges.length, 'edges');
            setTimeout(() => {
                console.log('⏳ Delayed edge processing starting...');

                // Check if DataSet has any leftover edges before processing
                const existingEdges = edges.get();
                if (existingEdges.length > 0) {
                    console.error('❌ DATASET STILL HAS EDGES AFTER CLEARING!');
                    console.error('Leftover edges:', existingEdges);
                    // Try to clear them again
                    existingEdges.forEach(edge => {
                        try {
                            edges.remove(edge.id);
                            console.log(`🗑️ Removed leftover edge: ${edge.id}`);
                        } catch (removeError) {
                            console.error(`❌ Failed to remove leftover edge ${edge.id}:`, removeError);
                        }
                    });
                }

                // Check for duplicate edges before processing
                const edgeIds = data.edges.map(edge => `${edge.source}--${edge.target}`);
                const uniqueEdgeIds = new Set(edgeIds);
                if (edgeIds.length !== uniqueEdgeIds.size) {
                    console.error('❌ DUPLICATE EDGES DETECTED IN SERVER RESPONSE!');
                    console.error('Edge IDs:', edgeIds);
                    const duplicates = edgeIds.filter((id, index) => edgeIds.indexOf(id) !== index);
                    console.error('Duplicate edge IDs:', [...new Set(duplicates)]);
                    console.error('Full edge data:', data.edges);
                }

            data.edges.forEach(edge => {
                const edgeId = `${edge.source}--${edge.target}`;

                // Check if edge already exists in the shared DataSet
                console.log(`🔍 Checking edge ${edgeId} in shared DataSet...`);
                const existingEdge = edges.get(edgeId);

                if (existingEdge) {
                    console.warn(`⚠️ EDGE ALREADY EXISTS! ID: ${edgeId} - skipping`);
                    console.warn('Existing edge:', existingEdge);
                    console.warn('Attempting to add:', edge);
                    console.warn('Current edges count:', edges.length);
                    return; // Skip this edge
                }

                console.log(`✅ Edge ${edgeId} is unique, adding...`);

                const edgeData = {
                    id: edgeId,
                    from: edge.source,
                    to: edge.target,
                    label: edge.type || '',
                    arrows: 'to',
                    color: '#00aaff'
                };

                // Add to the shared DataSet (network.body.data.edges and edges are the same object)
                try {
                    network.body.data.edges.add(edgeData);
                    intentEdges.set(edgeId, edge);
                    console.log(`✅ Added edge ${edgeId} to shared DataSet`);
                } catch (error) {
                    console.error(`❌ Failed to add edge ${edgeId}:`, error);
                    console.error('Edge data:', edgeData);
                    return; // Skip this edge
                }
            });
            }, 50); // 50ms delay to ensure clearing is complete
        }

        // Smooth network update with additional delay for edge processing
        setTimeout(() => {
            console.log('🔄 Final network update...');
        network.redraw();
        network.fit();
        }, 150); // Increased delay to account for edge processing delay

        updateGraphStats();
        updateGoalStatus('Graph generated successfully. Ready to generate plans.');

        const rootNodes = data.nodes ? data.nodes.filter(n => n.is_root).length : 0;
        const childNodes = data.nodes ? data.nodes.length - rootNodes : 0;

        addLogEntry(`📊 Rendered new graph with ${data.nodes ? data.nodes.length : 0} nodes`);
        if (rootNodes > 0) {
            addLogEntry(`👑 ${rootNodes} root intent(s) - orchestrates execution`);
            addLogEntry(`🔢 ${childNodes} child intents numbered 1-${childNodes} (top to bottom = execution sequence)`);
        } else {
            addLogEntry(`🔢 Nodes are numbered 1-N showing execution sequence (top to bottom)`);
        }
        addLogEntry(`💾 Graph automatically saved to local storage for future sessions`);
        if (graphHistory.size > 1) {
            addLogEntry(`📚 ${graphHistory.size - 1} previous graphs available in history`);
        }
        console.log(`✅ Graph replacement completed successfully: ${nodesAdded} nodes, ${data.edges ? data.edges.length : 0} edges`);

        // Store the new graph in history after it's fully processed
        console.log('💾 Storing new graph in history...');
        storeCurrentGraphInHistory();
    }

    function handlePlanGenerated(data) {
        // Only process plans for the current graph
        if (currentGraphId && data.graph_id !== currentGraphId) {
            console.log(`📊 Ignoring plan for graph ${data.graph_id}, current graph is ${currentGraphId}`);
            return;
        }

        executeBtn.disabled = false;
        updateGoalStatus('Plans generated successfully. Ready to execute.');
        addLogEntry(`Plan generated for intent ${data.intent_id}: ${data.plan_id}`);

        // Store plan information for later retrieval
        generatedPlans.set(data.intent_id, {
            plan_id: data.plan_id,
            rtfs_code: data.rtfs_code,
            intent_id: data.intent_id,
            graph_id: data.graph_id,
            timestamp: new Date().toISOString()
        });

        // Update the RTFS code display with the latest plan
        if (data.rtfs_code) {
            rtfsCodeElement.textContent = data.rtfs_code;
            if (window.Prism) {
            Prism.highlightElement(rtfsCodeElement);
        }
        }

        console.log(`📋 Stored plan for intent ${data.intent_id}:`, generatedPlans.get(data.intent_id));
    }

    function handleReadyForNext(data) {
        updateGoalStatus(`Ready for next step: ${data.next_step}`);
        addLogEntry(`Ready for next step: ${data.next_step}`);
    }

    // Plan details display function - now updates RTFS container
    function showPlanDetails(node) {
        const rtfsContainer = document.getElementById('rtfs-container');
        const rtfsTitle = rtfsContainer ? rtfsContainer.querySelector('h3') : null;
        const rtfsCode = document.getElementById('rtfs-code');

        console.log(`🔍 showPlanDetails called for node:`, node.id);
        console.log(`🔍 RTFS container found:`, !!rtfsContainer);
        console.log(`🔍 RTFS title found:`, !!rtfsTitle);
        console.log(`🔍 RTFS code found:`, !!rtfsCode);

        if (!rtfsContainer || !rtfsTitle || !rtfsCode) {
            console.error('RTFS container elements not found');
            console.error('Elements:', { rtfsContainer, rtfsTitle, rtfsCode });
            return;
        }

        // Get plan information from stored plans
        const storedPlan = generatedPlans.get(node.id);
        console.log(`🔍 Looking for plan with node.id: ${node.id}`);
        console.log(`📋 Available plans:`, Array.from(generatedPlans.keys()));
        console.log(`📄 Found stored plan:`, storedPlan);

        const planCodeText = storedPlan ? storedPlan.rtfs_code : (node.plan_body_preview || 'Plan code not available');

        // Clear previous content first
        rtfsCode.textContent = '';

        // Update RTFS container title to show which plan is selected
        rtfsTitle.textContent = `📄 Plan: ${node.original_label || node.label}`;

        // Set plan code with syntax highlighting
        rtfsCode.textContent = planCodeText;
        console.log(`📝 Setting RTFS plan code to:`, planCodeText);
        console.log(`📝 RTFS code element found:`, !!rtfsCode);
        console.log(`📝 RTFS code element content after setting:`, rtfsCode.textContent);

        if (window.Prism) {
            // Use setTimeout to ensure DOM is updated before highlighting
            setTimeout(() => {
                Prism.highlightElement(rtfsCode);
                console.log(`✨ Applied syntax highlighting`);
                console.log(`✨ RTFS code element content after highlighting:`, rtfsCode.textContent);
            }, 10);
        }

        // Scroll the RTFS container into view
        setTimeout(() => {
            rtfsContainer.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        }, 100);

        // Force a DOM update by triggering a reflow
        rtfsContainer.offsetHeight;

        // Add a temporary visual indicator to RTFS container
        rtfsContainer.style.border = '2px solid #00ff88';
        setTimeout(() => {
            rtfsContainer.style.border = '1px solid #444';
        }, 1000);

        addLogEntry(`📋 Displaying plan in RTFS container: ${node.original_label || node.label}`);
    }

    function hidePlanDetails() {
        // Reset RTFS container to default state when no plan is selected
        const rtfsContainer = document.getElementById('rtfs-container');
        const rtfsTitle = rtfsContainer ? rtfsContainer.querySelector('h3') : null;
        const rtfsCode = document.getElementById('rtfs-code');

        if (rtfsTitle) {
            rtfsTitle.textContent = '📄 RTFS Plan';
        }
        if (rtfsCode) {
            rtfsCode.textContent = '';
        }

        console.log(`📋 RTFS container reset to default state`);
    }

    function getNodeColor(status) {
        switch (status) {
            case 'Active':
            case 'active':
                return {
                    border: '#00aaff',
                    background: '#2a2a2a',
                    highlight: { border: '#00ffaa', background: '#3a3a3a' }
                };
            case 'Executing':
            case 'executing':
                return {
                    border: '#ffff00',
                    background: '#4a4a00',
                    highlight: { border: '#ffff88', background: '#5a5a00' }
                };
            case 'Completed':
            case 'completed':
                return {
                    border: '#00ff00',
                    background: '#004a00',
                    highlight: { border: '#00ff88', background: '#005a00' }
                };
            case 'Failed':
            case 'failed':
            case 'error':
                return {
                    border: '#ff0000',
                    background: '#4a0000',
                    highlight: { border: '#ff8888', background: '#5a0000' }
                };
            case 'Pending':
            case 'pending':
                return {
                    border: '#888888',
                    background: '#333333',
                    highlight: { border: '#aaaaaa', background: '#444444' }
                };
            case 'Paused':
            case 'paused':
                return {
                    border: '#ffaa00',
                    background: '#4a2a00',
                    highlight: { border: '#ffbb44', background: '#5a3a00' }
                };
            default:
                return {
                    border: '#cccccc',
                    background: '#333333',
                    highlight: { border: '#ffffff', background: '#444444' }
                };
        }
    }

    // Setup close button for plan details
    const closePlanBtn = document.getElementById('close-plan-btn');
    if (closePlanBtn) {
        closePlanBtn.addEventListener('click', hidePlanDetails);
    }

});
