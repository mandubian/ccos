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
    // Code panes and tabs
    const intentCodeElement = document.getElementById('intent-code');
    const planCodeElement = document.getElementById('rtfs-code');
    const graphCodeElement = document.getElementById('graph-code');
    const tabIntent = document.getElementById('tab-intent');
    const tabPlan = document.getElementById('tab-plan');
    const tabGraph = document.getElementById('tab-graph');
    // Top-level navigation buttons
    const navIntents = document.getElementById('nav-intents');
    const navArchitecture = document.getElementById('nav-architecture');
    const toggleFormatBtn = document.getElementById('toggle-format');
    const toggleWrapBtn = document.getElementById('toggle-wrap');
    const copyCodeBtn = document.getElementById('copy-code');
    const logEntriesElement = document.getElementById('log-entries');
    const goalStatusElement = document.getElementById('goal-status');
    const graphStatsElement = document.getElementById('graph-stats');
    const currentGraphIdSpan = document.getElementById('current-graph-id');
    const selectedIntentLabelSpan = document.getElementById('selected-intent-label');
    const activityIndicator = document.getElementById('activity-indicator');
    const activityText = document.getElementById('activity-text');
    
    // Activity helpers: show/hide the small status indicator in the header
    function showActivity(message = 'Working...') {
        try {
            if (activityText) activityText.textContent = message;
            if (activityIndicator) activityIndicator.classList.remove('activity-hidden');
        } catch (e) {
            console.warn('showActivity failed:', e);
        }
    }

    function hideActivity() {
        try {
            if (activityIndicator) activityIndicator.classList.add('activity-hidden');
        } catch (e) {
            console.warn('hideActivity failed:', e);
        }
    }

    // Architecture pane elements
    // Architecture top-level view elements
    const architectureView = document.getElementById('view-architecture');
    const architectureVizContainer = document.getElementById('architecture-visualization');
    const architectureInspector = document.getElementById('architecture-details');
    const architectureGenerated = document.getElementById('architecture-generated');
    const architectureRefreshBtn = document.getElementById('architecture-refresh');
    const architectureExportBtn = document.getElementById('architecture-export');
    const architectureAutoRefreshCheckbox = document.getElementById('architecture-auto-refresh');
    let architectureAutoInterval = null;
    const architectureStatusBadge = document.getElementById('architecture-status');
    // Instrumentation: track layout element sizes to detect unwanted growth
    const graphViz = document.getElementById('graph-visualization');
    const archViz = document.getElementById('architecture-visualization');
    let lastGraphDims = null;
    let lastArchDims = null;

    function logIfChanged(label, el, lastDimsRef) {
        if (!el) return lastDimsRef;
        const r = el.getBoundingClientRect();
        const dims = { w: Math.round(r.width), h: Math.round(r.height) };
        if (!lastDimsRef || lastDimsRef.w !== dims.w || lastDimsRef.h !== dims.h) {
            console.debug(`[layout] ${label} ->`, dims);
            return dims;
        }
        return lastDimsRef;
    }

    function clampGraphContainers() {
        try {
            const maxH = window.innerHeight - 160; // leave space for headers & bottom panel
            [graphViz, archViz].forEach(el => {
                if (!el) return;
                const r = el.getBoundingClientRect();
                if (r.height > maxH) {
                    el.style.maxHeight = maxH + 'px';
                    el.style.overflow = 'auto';
                }
            });
        } catch (_) {}
    }

    // ResizeObserver to catch intrinsic canvas growth
    if (window.ResizeObserver) {
        const ro = new ResizeObserver(() => {
            lastGraphDims = logIfChanged('graph-visualization', graphViz, lastGraphDims);
            lastArchDims = logIfChanged('architecture-visualization', archViz, lastArchDims);
            clampGraphContainers();
        });
        if (graphViz) ro.observe(graphViz);
        if (archViz) ro.observe(archViz);
    } else {
        // Fallback polling (low frequency)
        setInterval(() => {
            lastGraphDims = logIfChanged('graph-visualization', graphViz, lastGraphDims);
            lastArchDims = logIfChanged('architecture-visualization', archViz, lastArchDims);
            clampGraphContainers();
        }, 8000);
    }

    // Create separate vis network for architecture
    let architectureNetwork = null;
    let architectureNodes = null;
    let architectureEdges = null;
    let architectureSnapshot = null; // cached last snapshot
    let filteredCapabilities = [];
    let capabilityPage = 0;
    const CAP_PAGE_SIZE = 100;

    // Inspector elements (lazy query to avoid errors if structure changes)
    const inspectorTabsContainer = () => document.getElementById('arch-inspector-tabs');
    const capabilitySearchInput = () => document.getElementById('cap-search');
    const capabilityNamespaceSelect = () => document.getElementById('cap-filter-namespace');
    const capabilityProviderSelect = () => document.getElementById('cap-filter-provider');
    const capabilitiesTbody = () => document.getElementById('capabilities-tbody');
    const capabilitiesPagination = () => document.getElementById('capabilities-pagination');
    const securityPre = () => document.getElementById('arch-security-json');
    const delegationPre = () => document.getElementById('arch-delegation-json');
    const chainPre = () => document.getElementById('arch-chain-json');
    const runtimePre = () => document.getElementById('arch-runtime-json');
    const overviewContent = () => document.getElementById('arch-overview-content');

    function switchInspectorTab(tabName) {
        const buttons = document.querySelectorAll('.arch-itab');
        const panels = document.querySelectorAll('.arch-panel');
        buttons.forEach(b => b.classList.toggle('active', b.getAttribute('data-itab') === tabName));
        panels.forEach(p => p.classList.toggle('active', p.id === 'arch-panel-' + tabName));
    }

    function initInspectorTabs() {
        const container = inspectorTabsContainer();
        if (!container) return;
        container.addEventListener('click', (e) => {
            const btn = e.target.closest('.arch-itab');
            if (!btn) return;
            const tab = btn.getAttribute('data-itab');
            switchInspectorTab(tab);
            if (tab === 'capabilities') renderCapabilitiesTable();
        });
    }

    function populateOverview(snapshot) {
        const el = overviewContent();
        if (!el || !snapshot) return;
        const comp = snapshot.components || {};
        const cg = comp.intent_graph || {};
        const cm = snapshot.capability_marketplace || {};
        const del = snapshot.delegation || {};
                const meta = snapshot.meta || {};
                const warnings = Array.isArray(meta.warnings) ? meta.warnings : [];
                const degraded = meta.degraded === true;

                // Update status badge if present
                if (typeof architectureStatusBadge !== 'undefined' && architectureStatusBadge) {
                        architectureStatusBadge.textContent = degraded ? 'Degraded' : (warnings.length ? `${warnings.length} warning${warnings.length>1?'s':''}` : 'OK');
                        architectureStatusBadge.className = 'status-badge';
                        if (degraded) architectureStatusBadge.classList.add('status-degraded');
                        else if (warnings.length) architectureStatusBadge.classList.add('status-warning');
                        else architectureStatusBadge.classList.add('status-ok');
                }

                const warningsHtml = warnings.length ? `
                    <div class="arch-warnings">
                        <div class="arch-warnings-header">Heuristic Warnings (${warnings.length})</div>
                        <ul class="arch-warnings-list">${warnings.map(w => `<li>${escapeHtml(w)}</li>`).join('')}</ul>
                    </div>` : '';

                el.innerHTML = `
                        <div class="arch-overview-grid">
                            <div><strong>Generated:</strong> ${snapshot.generated_at || '—'}</div>
                            <div><strong>Version:</strong> ${snapshot.version}</div>
                            <div><strong>Intents Total:</strong> ${cg.total ?? '—'}</div>
                            <div><strong>Active:</strong> ${cg.active ?? '—'}</div>
                            <div><strong>Capabilities:</strong> ${cm.total ?? '—'}</div>
                            <div><strong>Delegation Enabled:</strong> ${del.enabled === true}</div>
                            <div><strong>Degraded:</strong> ${degraded}</div>
                            <div><strong>Warnings:</strong> ${warnings.length}</div>
                        </div>
                        ${warningsHtml}`;
    }

    function buildCapabilityFilters(snapshot) {
        const caps = snapshot.capabilities || [];
        const nsSet = new Set();
        const provSet = new Set();
        caps.forEach(c => { if (c.namespace) nsSet.add(c.namespace); if (c.provider_type) provSet.add(c.provider_type); });
        const nsSel = capabilityNamespaceSelect();
        const provSel = capabilityProviderSelect();
        if (nsSel && nsSel.options.length <= 1) {
            Array.from(nsSet).sort().forEach(ns => {
                const opt = document.createElement('option'); opt.value = ns; opt.textContent = ns; nsSel.appendChild(opt);
            });
        }
        if (provSel && provSel.options.length <= 1) {
            Array.from(provSet).sort().forEach(p => {
                const opt = document.createElement('option'); opt.value = p; opt.textContent = p; provSel.appendChild(opt);
            });
        }
    }

    function filterCapabilities() {
        if (!architectureSnapshot) return [];
        const searchVal = (capabilitySearchInput()?.value || '').toLowerCase();
        const nsFilter = capabilityNamespaceSelect()?.value || '';
        const provFilter = capabilityProviderSelect()?.value || '';
        const caps = architectureSnapshot.capabilities || [];
        return caps.filter(c => {
            if (nsFilter && c.namespace !== nsFilter) return false;
            if (provFilter && c.provider_type !== provFilter) return false;
            if (searchVal) {
                const txt = `${c.id} ${c.namespace} ${c.provider_type}`.toLowerCase();
                if (!txt.includes(searchVal)) return false;
            }
            return true;
        });
    }

    function renderCapabilitiesTable() {
        filteredCapabilities = filterCapabilities();
        const tbody = capabilitiesTbody();
        const pagEl = capabilitiesPagination();
        if (!tbody) return;
        tbody.innerHTML = '';
        capabilityPage = Math.max(0, Math.min(capabilityPage, Math.floor(filteredCapabilities.length / CAP_PAGE_SIZE)));
        const start = capabilityPage * CAP_PAGE_SIZE;
        const slice = filteredCapabilities.slice(start, start + CAP_PAGE_SIZE);
        for (const c of slice) {
            const tr = document.createElement('tr');
            tr.innerHTML = `<td>${c.id}</td><td>${c.namespace}</td><td>${c.provider_type}</td><td>${c.allowed_by_policy}</td>`;
            tbody.appendChild(tr);
        }
        if (pagEl) {
            if (filteredCapabilities.length > CAP_PAGE_SIZE) {
                pagEl.classList.remove('hidden');
                const totalPages = Math.ceil(filteredCapabilities.length / CAP_PAGE_SIZE);
                pagEl.innerHTML = '';
                const prev = document.createElement('button'); prev.textContent = 'Prev'; prev.disabled = capabilityPage === 0; prev.onclick = () => { capabilityPage--; renderCapabilitiesTable(); };
                const next = document.createElement('button'); next.textContent = 'Next'; next.disabled = capabilityPage >= totalPages - 1; next.onclick = () => { capabilityPage++; renderCapabilitiesTable(); };
                const stats = document.createElement('span'); stats.textContent = `Page ${capabilityPage + 1}/${totalPages} (${filteredCapabilities.length} items)`;
                pagEl.append(prev, stats, next);
            } else { pagEl.classList.add('hidden'); }
        }
    }

    function populateSecurity(snapshot) { if (securityPre()) securityPre().textContent = JSON.stringify(snapshot.security, null, 2); }
    function populateDelegation(snapshot) { if (delegationPre()) delegationPre().textContent = JSON.stringify(snapshot.delegation, null, 2); }
    function populateRuntime(snapshot) { if (runtimePre()) runtimePre().textContent = JSON.stringify(snapshot.components?.rtfs_runtime || {}, null, 2); }
    function populateChain(snapshot) { if (chainPre()) chainPre().textContent = JSON.stringify(snapshot.components?.causal_chain || {}, null, 2); }

    function wireCapabilityFilters() {
        const inputs = [capabilitySearchInput(), capabilityNamespaceSelect(), capabilityProviderSelect()];
        inputs.forEach(inp => inp && inp.addEventListener('input', () => { capabilityPage = 0; renderCapabilitiesTable(); }));
        inputs.forEach(sel => sel && sel.addEventListener('change', () => { capabilityPage = 0; renderCapabilitiesTable(); }));
    }

    function ensureArchitectureNetwork() {
        if (architectureNetwork) return;
        architectureNodes = new vis.DataSet([]);
        architectureEdges = new vis.DataSet([]);
        const data = { nodes: architectureNodes, edges: architectureEdges };
        const opts = {
            layout: {
                hierarchical: {
                    enabled: true,
                    direction: 'LR',
                    sortMethod: 'hubsize',
                    levelSeparation: 180,
                    nodeSpacing: 160,
                    treeSpacing: 200,
                },
                improvedLayout: true
            },
            physics: { enabled: false }, // deterministic positioning
            groups: {
                arbiter: { color: { background: '#222', border: '#ffcc00' }, shape: 'box' },
                governance: { color: { background: '#202830', border: '#00b4ec' }, shape: 'box' },
                orchestrator: { color: { background: '#262626', border: '#ffaa00' }, shape: 'box' },
                store: { color: { background: '#1d1d1d', border: '#888' }, shape: 'database' },
                ledger: { color: { background: '#281d1d', border: '#d45555' }, shape: 'box' },
                marketplace: { color: { background: '#1d281d', border: '#4caf50' }, shape: 'box' },
                storage: { color: { background: '#222222', border: '#666' }, shape: 'box' },
                runtime: { color: { background: '#1f2430', border: '#8f7fff' }, shape: 'box' },
                default: { color: { background: '#222', border: '#00aaff' }, shape: 'ellipse' }
            },
            nodes: { font: { color: '#fff', size: 14 }, margin: 10 },
            edges: {
                arrows: { to: { enabled: true, scaleFactor: 0.8 } },
                smooth: { type: 'cubicBezier', forceDirection: 'horizontal', roundness: 0.4 },
                width: 1.5,
                color: { color: '#00aaff', highlight: '#ffffff', hover: '#ffffff' },
                font: { color: '#ccc', strokeWidth: 0, size: 10, background: 'transparent' }
            },
            interaction: { hover: true, navigationButtons: false, keyboard: false }
        };
        try {
            architectureNetwork = new vis.Network(architectureVizContainer, data, opts);
            architectureNetwork.on('click', (params) => {
                if (params.nodes && params.nodes.length > 0) {
                    const id = params.nodes[0];
                    const node = architectureNodes.get(id);
                    showArchitectureInspector(node);
                }
            });
                // When edges clicked, show relation description in inspector
                architectureNetwork.on('click', (params) => {
                    if (params.edges && params.edges.length > 0) {
                        const edgeId = params.edges[0];
                        const edge = architectureEdges.get(edgeId);
                        if (edge) {
                            showEdgeRelationInInspector(edge);
                        }
                    }
                });
        } catch (e) {
            console.error('Failed to initialize architecture network', e);
            addLogEntry('error', 'Failed to initialize architecture network: ' + e.message);
        }
    }

    function showArchitectureInspector(node) {
        if (!architectureInspector) return;
        if (!node) {
            architectureInspector.innerHTML = 'No node selected';
            return;
        }
        const lines = [];
        lines.push('<strong>' + (node.label || node.id) + '</strong>');
        if (node.group) lines.push('<div><em>Group:</em> ' + node.group + '</div>');
        if (node.present !== undefined) lines.push('<div><em>Present:</em> ' + node.present + '</div>');
        if (node.details) lines.push('<pre>' + JSON.stringify(node.details, null, 2) + '</pre>');
        architectureInspector.innerHTML = lines.join('\n');
    }

    async function fetchAndRenderArchitecture() {
        showActivity('Loading architecture...');
        try {
            const resp = await fetch('/architecture?include=capabilities');
            if (!resp.ok) {
                const txt = await resp.text();
                addLogEntry('error', 'Failed to fetch architecture: ' + txt);
                updateGoalStatus('Failed to fetch architecture');
                hideActivity();
                return;
            }
            const data = await resp.json();
            architectureSnapshot = data; // cache
            if (architectureGenerated) architectureGenerated.textContent = data.generated_at || '—';
            ensureArchitectureNetwork();
            // Build nodes and edges
            const nodesArr = (data.graph_model && data.graph_model.nodes) || [];
            const edgesArr = (data.graph_model && data.graph_model.flow_edges) || [];

            // Assign levels for hierarchical layout (simple mapping by component role)
            const levelMap = {
                arbiter: 0,
                delegating_arbiter: 0,
                governance_kernel: 1,
                orchestrator: 2,
                intent_graph: 3,
                capability_marketplace: 3,
                causal_chain: 3,
                plan_archive: 4,
                rtfs_runtime: 2
            };

            const visNodes = nodesArr.map(n => ({
                id: n.id,
                label: n.label,
                group: n.group || 'default',
                title: n.label,
                details: n,
                level: levelMap[n.id] !== undefined ? levelMap[n.id] : undefined,
                shape: undefined
            }));

            const relationStyles = {
                proposes_plan: { color: '#ffaa00' },
                validated_plan: { color: '#00b4ec' },
                dispatches_calls: { color: '#4caf50' },
                updates_intents: { color: '#8f7fff' },
                creates_intents: { color: '#ffcc00' },
                records_actions: { color: '#d45555' },
                records_validation: { color: '#d45555' }
            };

            const visEdges = edgesArr.map(e => {
                const style = relationStyles[e.relation] || {};
                return {
                    from: e.from,
                    to: e.to,
                    arrows: 'to',
                    title: e.relation,
                    label: e.relation.replace(/_/g, '\n'),
                    color: style.color || '#00aaff'
                };
            });
            architectureNodes.clear(); architectureEdges.clear();
            architectureNodes.add(visNodes); architectureEdges.add(visEdges);
            architectureNetwork.fit({ animation: false });
            // Apply mapping classes for special groups so CSS can style them
            // vis.js renders canvas nodes; we can set 'className' on nodes to help with DOM overlays
            visNodes.forEach(n => {
                if (n.group === 'input' || n.group === 'llm') {
                    architectureNodes.update({ id: n.id, className: n.group });
                }
            });
            // Populate legend dynamically
            const legend = document.getElementById('architecture-legend');
            if (legend) {
                legend.innerHTML = '';
                const items = [
                    { color: '#00ff88', label: 'User Goal (input)' },
                    { color: '#ffcc00', label: 'Arbiter / Orchestration' },
                    { color: '#00b4ec', label: 'Governance' },
                    { color: '#8f7fff', label: 'Runtime / Delegation (LLM)' },
                    { color: '#4caf50', label: 'Capability Marketplace' },
                ];
                items.forEach(it => {
                    const el = document.createElement('div'); el.className = 'legend-item';
                    const sw = document.createElement('div'); sw.className = 'legend-swatch'; sw.style.background = it.color;
                    const lab = document.createElement('div'); lab.className = 'legend-label'; lab.textContent = it.label;
                    el.appendChild(sw); el.appendChild(lab); legend.appendChild(el);
                });
            }
            // Populate panels
            populateOverview(data);
            buildCapabilityFilters(data);
            renderCapabilitiesTable();
            populateSecurity(data);
            populateDelegation(data);
            populateRuntime(data);
            populateChain(data);
            addLogEntry('info', 'Architecture snapshot loaded');
            enforceBottomPanelBounds();
        } catch (e) {
            console.error('Error fetching architecture', e);
            addLogEntry('error', 'Error fetching architecture: ' + e.message);
            updateGoalStatus('Error fetching architecture');
        } finally {
            hideActivity();
        }
    }

    function showEdgeRelationInInspector(edge) {
        if (!architectureInspector) return;
        const rel = edge.label || edge.title || edge.relation || 'relation';
        // Map common relations to friendly descriptions
        const descMap = {
            'proposes_plan': 'Arbiter proposes a plan (NL -> Plan)',
            'validated_plan': 'Governance Kernel validates and sanitizes plans',
            'dispatches_calls': 'Orchestrator dispatches calls to capabilities',
            'updates_intents': 'Orchestrator updates intent statuses',
            'creates_intents': 'Arbiter creates intents in the intent graph',
            'records_actions': 'Actions recorded to the causal chain for auditing',
            'records_validation': 'Governance validation recorded to ledger',
            'delegates': 'Arbiter delegates planning/interpretation to Delegating Arbiter',
            'llm_call': 'Delegating Arbiter calls external LLM provider for plan synthesis',
            'submits_goal': 'User submits natural language goal to Arbiter'
        };
        const desc = descMap[rel] || rel;
        architectureInspector.innerHTML = `<strong>Relation:</strong> ${rel}<div style="margin-top:6px;">${desc}</div>`;
    }

    if (architectureRefreshBtn) architectureRefreshBtn.addEventListener('click', () => fetchAndRenderArchitecture());
    const architectureToggleGraphBtn = document.getElementById('architecture-toggle-graph');
    if (architectureToggleGraphBtn && architectureVizContainer) {
        // Persist toggle state in localStorage
        const KEY = 'ccos_arch_graph_visible';
        function applyToggleState() {
            const visible = localStorage.getItem(KEY) !== 'false';
            if (visible) {
                architectureVizContainer.style.display = '';
                architectureToggleGraphBtn.classList.remove('inactive');
                architectureToggleGraphBtn.textContent = '🧩 Graph';
            } else {
                architectureVizContainer.style.display = 'none';
                architectureToggleGraphBtn.classList.add('inactive');
                architectureToggleGraphBtn.textContent = '🧩 Graph (hidden)';
            }
        }
        applyToggleState();
        architectureToggleGraphBtn.addEventListener('click', () => {
            const currentlyVisible = architectureVizContainer.style.display !== 'none';
            localStorage.setItem(KEY, currentlyVisible ? 'false' : 'true');
            applyToggleState();
            if (!currentlyVisible) {
                ensureArchitectureNetwork();
                if (architectureNodes.length && architectureNodes.length() === 0) {
                    fetchAndRenderArchitecture();
                } else if (architectureNetwork) {
                    setTimeout(() => architectureNetwork.fit({ animation: false }), 0);
                }
            }
        });
    }
    initInspectorTabs();
    wireCapabilityFilters();

    if (architectureExportBtn) {
        architectureExportBtn.addEventListener('click', () => {
            if (!architectureSnapshot) { addLogEntry('warning', 'No snapshot to export'); return; }
            const blob = new Blob([JSON.stringify(architectureSnapshot, null, 2)], { type: 'application/json' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            const ts = architectureSnapshot.generated_at || new Date().toISOString();
            a.download = `ccos_architecture_${ts.replace(/[:]/g,'-')}.json`;
            document.body.appendChild(a); a.click(); document.body.removeChild(a);
            URL.revokeObjectURL(url);
        });
    }

    function startArchitectureAutoRefresh() {
        if (architectureAutoInterval) return;
        architectureAutoInterval = setInterval(() => {
            if (!document.getElementById('view-architecture')?.classList.contains('active')) return; // only refresh when visible
            fetchAndRenderArchitecture();
        }, 15000);
    }
    function stopArchitectureAutoRefresh() {
        if (architectureAutoInterval) { clearInterval(architectureAutoInterval); architectureAutoInterval = null; }
    }
    if (architectureAutoRefreshCheckbox) {
        architectureAutoRefreshCheckbox.addEventListener('change', (e) => {
            if (e.target.checked) startArchitectureAutoRefresh(); else stopArchitectureAutoRefresh();
        });
    }

    // Tab activation for Architecture
    // ---------------- Top-Level View Routing ----------------
    function activateView(name) {
        const views = document.querySelectorAll('.top-view');
        views.forEach(v => v.classList.remove('active'));
        if (name === 'architecture' && architectureView) {
            architectureView.classList.add('active');
            if (navIntents) navIntents.classList.remove('active');
            if (navArchitecture) navArchitecture.classList.add('active');
            ensureArchitectureNetwork();
            // Only fetch if we have no nodes yet
            if (!architectureNodes || architectureNodes.length === 0 || (architectureNodes.length && architectureNodes.length() === 0)) {
                fetchAndRenderArchitecture();
            }
        } else {
            // default fallback is intents view
            const intentsView = document.getElementById('view-intents');
            if (intentsView) intentsView.classList.add('active');
            if (navArchitecture) navArchitecture.classList.remove('active');
            if (navIntents) navIntents.classList.add('active');
        }
        // Update hash (suppress duplicate writes)
        const targetHash = '#' + (name === 'architecture' ? 'architecture' : 'intents');
        if (window.location.hash !== targetHash) {
            history.replaceState(null, '', targetHash);
        }
    }

    function handleHashNavigation() {
        const h = window.location.hash.replace('#', '');
        if (h === 'architecture') {
            activateView('architecture');
        } else {
            activateView('intents');
        }
    }

    if (navArchitecture) {
        navArchitecture.addEventListener('click', () => activateView('architecture'));
    }
    if (navIntents) {
        navIntents.addEventListener('click', () => activateView('intents'));
    }
    window.addEventListener('hashchange', handleHashNavigation);
    // Initialize based on current hash
    handleHashNavigation();
    const logFilter = document.getElementById('log-filter');
    const clearLogsBtn = document.getElementById('clear-logs');
    const toggleLogsBtn = document.getElementById('toggle-logs');
    const logContainer = document.getElementById('log-container');
    const logResizeHandle = document.getElementById('log-resize-handle');

    // Activity Log: collapsed by default, expandable/resizable via CSS class
    if (logContainer) {
        // Expand logs by default so activity is visible immediately
        logContainer.classList.add('logs-expanded');
    }
    if (toggleLogsBtn && logContainer) {
        // Initialize button text correctly (collapsed = Show)
        // Since we expand by default, initialize button as Hide
        toggleLogsBtn.textContent = '📋 Hide';
        toggleLogsBtn.title = 'Hide Log Panel';
        
        toggleLogsBtn.addEventListener('click', () => {
            logContainer.classList.toggle('logs-expanded');
            // Update button text to reflect current state
            const isExpanded = logContainer.classList.contains('logs-expanded');
            toggleLogsBtn.textContent = isExpanded ? '📋 Hide' : '📋 Show';
            toggleLogsBtn.title = isExpanded ? 'Hide Log Panel' : 'Show Log Panel';
            
            if (!isExpanded) {
                // Collapsing: Clear inline styles to allow CSS to take over
                logContainer.style.removeProperty('height');
                logContainer.style.removeProperty('max-height');
                logContainer.style.removeProperty('flex');
                logContainer.style.removeProperty('flex-grow');
                logContainer.style.removeProperty('flex-shrink');
            } else {
                // Expanding: Restore custom height if available
                if (lastCustomHeight) {
                    logContainer.style.setProperty('height', lastCustomHeight + 'px', 'important');
                    logContainer.style.setProperty('max-height', 'none', 'important');
                    logContainer.style.setProperty('flex', 'none', 'important');
                    logContainer.style.setProperty('flex-grow', '0', 'important');
                    logContainer.style.setProperty('flex-shrink', '0', 'important');
                }
            }
        });
        
        // Removed drag-to-resize code and handle; logs are toggled via the button and CSS only.
    }

    // ---- Bottom panel integration ----
    const bottomPanel = document.getElementById('bottom-panel');
    const bottomTabExec = document.getElementById('bottom-tab-exec');
    const bottomTabActivity = document.getElementById('bottom-tab-activity');
    const bottomPanelExec = document.getElementById('bottom-panel-exec');
    const bottomPanelActivity = document.getElementById('bottom-panel-activity');
    const bottomPanelToggle = document.getElementById('bottom-panel-toggle');
    const bottomPanelResize = document.getElementById('bottom-panel-resize');

    function showBottomPanel() {
        if (!bottomPanel) return;
        bottomPanel.classList.remove('hidden');
        bottomPanel.classList.remove('collapsed');
        // Defensive: some elements may be absent in older builds; guard access
        if (bottomTabExec && bottomTabExec.classList) bottomTabExec.classList.add('active');
        if (bottomPanelExec && bottomPanelExec.classList) bottomPanelExec.classList.remove('hidden');
        if (bottomPanelActivity && bottomPanelActivity.classList) bottomPanelActivity.classList.add('hidden');
        // Keep body padding in sync
        // Panel is shown; do not modify document body padding so the panel stays in-flow
        if (bottomPanelToggle) bottomPanelToggle.textContent = '▼/▲';
    }

    function hideBottomPanel() {
        if (!bottomPanel) return;
        // collapse the panel visually but keep a small bar visible so user can expand it
        bottomPanel.classList.remove('hidden');
        bottomPanel.classList.add('collapsed');
        // Panel collapsed: do not change body padding; panel remains in document flow and will scroll with page
        if (bottomPanelToggle) bottomPanelToggle.textContent = '▲/▼';
    }

    if (bottomTabExec && bottomTabActivity && bottomPanelExec && bottomPanelActivity) {
        bottomTabExec.addEventListener('click', () => {
            bottomTabExec.classList.add('active');
            bottomTabActivity.classList.remove('active');
            bottomPanelExec.classList.remove('hidden');
            bottomPanelActivity.classList.add('hidden');
        });
        bottomTabActivity.addEventListener('click', () => {
            bottomTabActivity.classList.add('active');
            bottomTabExec.classList.remove('active');
            bottomPanelActivity.classList.remove('hidden');
            bottomPanelExec.classList.add('hidden');
        });
    }

    if (bottomPanelToggle) {
        bottomPanelToggle.addEventListener('click', () => {
            if (!bottomPanel) return;
            // Toggle collapsed state (keep element present so a small bar remains visible)
            const isCollapsed = bottomPanel.classList.toggle('collapsed');
            if (isCollapsed) {
                // Keep panel in flow; nothing to change on body
                bottomPanelToggle.textContent = '▲/▼';
            } else {
                bottomPanelToggle.textContent = '▼/▲';
            }
        });
    }

    if (bottomPanelResize && bottomPanel) {
        let resizing = false;
        let startY = 0;
        let startHeight = 0;
        bottomPanelResize.addEventListener('mousedown', (e) => {
            resizing = true;
            startY = e.clientY;
            startHeight = bottomPanel.offsetHeight;
            document.body.style.cursor = 'ns-resize';
            e.preventDefault();
        });
        document.addEventListener('mousemove', (e) => {
            if (!resizing) return;
            const delta = startY - e.clientY;
            let newH = Math.max(80, Math.min(window.innerHeight * 0.8, startHeight + delta));
            bottomPanel.style.height = newH + 'px';
            e.preventDefault();
        });
        document.addEventListener('mouseup', () => {
            if (resizing) {
                resizing = false;
                document.body.style.cursor = '';
            }
        });
    }

    // Expose for other modules to show execution panel when new data arrives
    window.showExecutionPanel = function(){
        showBottomPanel();
        // ensure body padding is set when programmatically opening
        if (bottomPanel && !bottomPanel.classList.contains('collapsed')) {
            if (bottomPanelToggle) bottomPanelToggle.textContent = '▼/▲';
        }
    };

    // Make the bottom panel visible by default so activity logs are always accessible.
    try { showBottomPanel(); } catch (e) { console.warn('showBottomPanel failed on init:', e); }

    // Helper function for node badges (placeholder)
    function getBadgeText(nodeId) {
        return ''; // No badges for now
    }

    // Update visual indicator on a node when a plan is available (or removed)
    function updateNodePlanIndicator(nodeId, hasPlan) {
        if (!nodeId) return;
        const node = nodes.get(nodeId);
        if (!node) {
            console.warn(`⚠️ updateNodePlanIndicator: node not found: ${nodeId}`);
            return;
        }

        const nodeUpdate = { id: nodeId };
        if (hasPlan) {
            // mark as having a plan
            nodeUpdate.has_plan = true;
            nodeUpdate.original_label = node.original_label || node.label;
            let newLabel = nodeUpdate.original_label || node.label;
            if (newLabel && !newLabel.includes('📋')) newLabel = newLabel + ' 📋';
            nodeUpdate.label = newLabel;
            nodeUpdate.plan_id = node.plan_id || null;
            nodeUpdate.color = {
                border: '#00ff88',
                background: node.color?.background || '#2a2a2a',
                highlight: { border: '#88ffaa', background: '#3a3a3a' }
            };
            nodeUpdate.title = `${node.original_label || node.label}\n📋 Has Plan Available\nClick to view plan details`;
        } else {
            // remove plan indicator
            nodeUpdate.has_plan = false;
            nodeUpdate.plan_id = null;
            nodeUpdate.label = node.original_label || node.label;
            nodeUpdate.color = node.color || getNodeColor(node.status || 'pending');
            nodeUpdate.title = node.original_label || node.label;
        }

        try {
            nodes.update(nodeUpdate);
        } catch (e) {
            console.error(`❌ updateNodePlanIndicator failed for ${nodeId}:`, e);
        }
    }

    // State management
    let selectedIntentId = null;
    let intentNodes = new Map(); // id -> node data
    let intentEdges = new Map(); // id -> edge data
    let generatedPlans = new Map(); // intent_id -> plan data
    let isWrapMode = true; // true for wrap, false for no-wrap
    let logEntries = []; // array of log entries
    let logFilterLevel = 'all'; // current log filter level
    let isLogsVisible = false; // logs collapsed by default

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
        try {
            // Attempt to persist any current graph and plans before the page unloads
            try { storeCurrentGraphInHistory(); } catch (e) { console.warn('⚠️ storeCurrentGraphInHistory failed during unload:', e); }
            try { saveGraphHistoryToStorage(); } catch (e) { console.warn('⚠️ saveGraphHistoryToStorage failed during unload:', e); }
        } catch (e) {}
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
    // Ensure code panes size correctly on initial load and when window resizes
    try {
        updateCodePaneHeights();
        window.addEventListener('resize', updateCodePaneHeights);
        window.addEventListener('resize', enforceBottomPanelBounds);
        // Initial clamp
        enforceBottomPanelBounds();
    } catch (e) {
        console.warn('⚠️ Failed to initialize code pane height handler:', e);
    }

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
                addLogEntry('error', 'Error: Form elements not found');
                return;
            }

            const goal = input.value.trim();

            // Input validation
            if (!goal) {
                addLogEntry('error', 'Error: Please enter a goal before submitting');
                updateGoalStatus('Please enter a goal');
                input.focus();
                return;
            }

            if (goal.length < 10) {
                addLogEntry('warning', 'Warning: Goal must be at least 10 characters long');
                updateGoalStatus('Goal must be more descriptive');
                input.focus();
                return;
            }

            if (goal.length > 1000) {
                addLogEntry('warning', 'Warning: Goal must be less than 1000 characters');
                updateGoalStatus('Goal is too long, please shorten it');
                return;
            }

            // Disable form during submission
            submitBtn.disabled = true;
            submitBtn.textContent = 'Generating...';
            input.disabled = true;
            
            // Show activity indicator
            showActivity('Generating intent graph...');
            addLogEntry('info', `🚀 Generating graph for goal: "${goal.substring(0, 50)}..."`);
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
        
        console.log('📚 populateGraphHistorySelector: graphHistory size=', graphHistory.size);
        console.log('📚 populateGraphHistorySelector: keys=', Array.from(graphHistory.keys()));
        sortedGraphs.forEach(([graphId, graph]) => {
            const option = document.createElement('option');
            option.value = graphId;
            const timeStr = graph.timestamp.toLocaleString();
            // Support both Map (runtime) and Array (legacy/serialized) representations for plans
            let planCount = 0;
            let planType = typeof graph.plans;
            if (graph.plans) {
                if (typeof graph.plans.size === 'number') {
                    planCount = graph.plans.size;
                    planType = 'Map';
                } else if (Array.isArray(graph.plans)) {
                    planCount = graph.plans.length;
                    planType = 'Array';
                } else if (typeof graph.plans.length === 'number') {
                    planCount = graph.plans.length;
                    planType = 'ArrayLike';
                }
            }
            console.log(`📚 populateGraphHistorySelector: graphId=${graphId} planType=${planType} planCount=${planCount}`);

            // Fallback: if planCount is zero, check generatedPlans for plans that belong to this graph (by rootId)
            if (planCount === 0 && typeof graph.rootId === 'string') {
                try {
                    const fallback = Array.from(generatedPlans.values()).filter(p => p && p.graph_id === graph.rootId).length;
                    if (fallback > 0) {
                        console.log(`🔎 populateGraphHistorySelector: fallback found ${fallback} plans for graph.rootId=${graph.rootId}`);
                        planCount = fallback;
                    }
                } catch (e) {
                    console.warn('⚠️ populateGraphHistorySelector fallback counting failed:', e);
                }
            }

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
        
        const historicalGraph = graphHistory.get(selectedGraphId);
        if (!historicalGraph) {
            addLogEntry('❌ Selected graph not found in history');
            return;
        }
        
        const success = await restoreGraphFromHistory(selectedGraphId);
        if (success) {
            addLogEntry(`✅ Loaded graph "${historicalGraph.name}" from history`);
            updateGoalStatus('Graph loaded from history. Ready to generate plans or execute.');
            
            // Update UI state
            if (generatePlansBtn) generatePlansBtn.disabled = false;
            if (executeBtn) executeBtn.disabled = generatedPlans.size === 0;
            
            // Clear selection
            graphHistorySelector.value = '';
            updateGraphHistoryButtons();
        } else {
            addLogEntry(`❌ Failed to load graph "${historicalGraph.name}" from history`);
            // Clear selection even on failure
            graphHistorySelector.value = '';
            updateGraphHistoryButtons();
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
                addLogEntry('error', '❌ Error: No graph available. Generate a graph first.');
                updateGoalStatus('No graph available - generate a graph first');
                return;
            }

            const originalText = generatePlansBtn.textContent;
            generatePlansBtn.disabled = true;
            generatePlansBtn.textContent = 'Generating...';
            
            // Show activity indicator
            showActivity('Generating plans...');
            addLogEntry('info', '📋 Generating plans for all intents...');
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
                // Update code panes when logs toggle
                updateCodePaneHeights();
                generatedPlans.clear();

            // Clear UI
                if (intentCodeElement) intentCodeElement.textContent = '';
                if (planCodeElement) planCodeElement.textContent = '';
                if (graphCodeElement) graphCodeElement.textContent = '';
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
                    // legacy: removed rtfsCode in favor of eager events
                    rtfsCode: graphCodeElement ? graphCodeElement.textContent : '',
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
                                const hasPlan = generatedPlans.has(node.id);
                                const labelText = node.label || node.id;
                                const planIndicator = hasPlan ? ' 📋' : '';
                                
                                const nodeData = {
                                    id: node.id,
                                    label: labelText + planIndicator,
                                    color: hasPlan ? {
                                        border: '#00ff88',
                                        background: '#2a2a2a',
                                        highlight: { border: '#00ff88', background: '#3a3a3a' }
                                    } : getNodeColor(node.status || 'pending'),
                                    borderWidth: hasPlan ? 3 : 2,
                                    title: `${node.label || node.id}\nStatus: ${node.status || 'pending'}\nType: ${node.type || 'unknown'}${getBadgeText(node.id)}`
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

                            // Load RTFS code if available
                            if (graphData.rtfsCode && graphCodeElement) {
                                graphCodeElement.textContent = graphData.rtfsCode;
                                if (typeof Prism !== 'undefined') {
                                    Prism.highlightElement(graphCodeElement);
                                }
                            }

                            // Update UI
                            updateGraphStats();
                            // Use non-animated fit to avoid animation-induced layout jitter
                            network.fit();

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
                                        
                                        // Load metadata after server registration
                                        if (graphData.metadata) {
                                            addLogEntry(`📁 Graph loaded: ${graphData.metadata.nodeCount || nodes.length} nodes, ${graphData.metadata.edgeCount || edges.length} edges`);
                                        } else {
                                            addLogEntry(`📁 Graph loaded: ${nodes.length} nodes, ${edges.length} edges`);
                                        }
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

                // On double click, focus and render according to active tab
                const nodePosition = network.getPositions([nodeId])[nodeId];
                if (nodePosition) {
                    network.moveTo({
                        position: nodePosition,
                        scale: 1.5,
                        animation: { duration: 500, easingFunction: 'easeInOutQuad' }
                    });
                }
                selectedIntentId = nodeId;
                if (isTabActive('plan') && node && node.has_plan) {
                    renderPlanRtfs(nodeId);
                } else {
                    activateTab('intent');
                    renderIntentRtfs(nodeId);
                }
                selectIntent(nodeId);
                addLogEntry(`🎯 Focused node: ${intentNodes.get(nodeId)?.label || nodeId}`);
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

                // Single click: select and render according to active tab
                selectedIntentId = nodeId;
                if (isTabActive('plan') && node && node.has_plan) {
                    renderPlanRtfs(nodeId);
                } else {
                    activateTab('intent');
                    renderIntentRtfs(nodeId);
                }
                selectIntent(nodeId);
            } else {
                // Clicked on empty space: keep panes as-is
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
        selectedIntentId = intentId;
        const node = intentNodes.get(intentId);
        if (node) {
            addLogEntry('info', `📋 Selected intent: ${node.label || node.id}`);
            // Re-render current tab to reflect selection
            if (isTabActive('intent')) renderIntentRtfs(intentId);
            if (isTabActive('plan')) renderPlanRtfs(intentId);
            // Update status bar
            updateStatusBar();
        } else {
            addLogEntry('warning', `⚠️ Intent ${intentId} not found in current graph data`);
        }
    }

    // Safely update the small status bar region (current graph + selected intent label)
    // This was referenced before being defined, causing a ReferenceError. We implement
    // a defensive version here so calls earlier in the code path succeed regardless
    // of load timing.
    function updateStatusBar() {
        try {
            if (currentGraphIdSpan) {
                currentGraphIdSpan.textContent = currentGraphId ? currentGraphId : '—';
            }
            if (selectedIntentLabelSpan) {
                if (selectedIntentId) {
                    const node = intentNodes.get(selectedIntentId);
                    const label = node?.label || node?.goal || selectedIntentId;
                    selectedIntentLabelSpan.textContent = label;
                    selectedIntentLabelSpan.title = label;
                } else {
                    selectedIntentLabelSpan.textContent = 'None';
                    selectedIntentLabelSpan.title = 'No intent selected';
                }
            }
            // Optionally reflect counts (nodes/edges) if elements exist in future
            if (graphStatsElement) {
                const nodeCount = nodes.length;
                const edgeCount = edges.length;
                graphStatsElement.textContent = `${nodeCount} intents, ${edgeCount} relationships`;
            }
        } catch (e) {
            console.error('Error updating status bar:', e);
        }
    }

    // Update the goal status small text in the UI (defensive, non-throwing)
    function updateGoalStatus(message) {
        try {
            if (goalStatusElement) {
                goalStatusElement.textContent = message || '';
            } else {
                // If the element isn't available yet, log as a fallback so callers still get observability
                console.log('goalStatus:', message);
            }
        } catch (e) {
            console.warn('⚠️ updateGoalStatus failed:', e);
        }
    }

    // Ensure code panes (<pre> elements) fill the available height of their container without causing cumulative growth.
    function updateCodePaneHeights() {
        try {
            const panesContainer = document.getElementById('code-panes');
            if (!panesContainer) return;
            const tabsBar = document.getElementById('code-tabs');
            const parent = panesContainer.parentElement; // typically the RTFS container
            if (!parent) return;
            const parentRect = parent.getBoundingClientRect();
            const tabsH = tabsBar ? tabsBar.getBoundingClientRect().height : 0;
            // Padding fudge factor (matches internal spacing) so we don't overflow.
            const available = Math.floor(parentRect.height - tabsH - 8);
            if (available > 120) { // only apply if we have a sensible space
                panesContainer.style.height = available + 'px';
                const pres = panesContainer.querySelectorAll('pre');
                const innerMax = available - 16; // subtract a little for padding
                pres.forEach(p => {
                    p.style.maxHeight = innerMax + 'px';
                    p.style.minHeight = '0px';
                });
            }
        } catch (e) {
            console.warn('updateCodePaneHeights failed:', e);
        }
    }

    // Clamp bottom panel height defensively to its CSS max (60vh) in case any incremental layout jitter accumulates.
    function enforceBottomPanelBounds() {
        try {
            const bp = document.getElementById('bottom-panel');
            if (!bp || bp.classList.contains('collapsed')) return;
            const maxPx = Math.round(window.innerHeight * 0.60);
            const hStr = bp.style.height || window.getComputedStyle(bp).height;
            const current = parseInt(hStr, 10);
            if (!isNaN(current) && current > maxPx) {
                bp.style.height = maxPx + 'px';
            }
        } catch (e) {
            // Silent: purely defensive
        }
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
            case 'ExecutionStarted':
                handleExecutionStarted(event.data);
                break;
            case 'IntentExecution':
                handleIntentExecution(event.data);
                break;
            case 'ExecutionFinished':
                handleExecutionFinished(event.data);
                break;
            case 'GraphGenerated':
                console.log('📡 RECEIVED GraphGenerated event:', event.data);
                console.log('🔍 WebSocket message sequence check - timestamp:', Date.now());
                handleGraphGenerated(event.data);
                break;
            case 'PlanGenerated':
                handlePlanGenerated(event.data);
                break;
            case 'IntentRtfsGenerated':
                handleIntentRtfsGenerated(event.data);
                break;
            case 'GraphRtfsGenerated':
                handleGraphRtfsGenerated(event.data);
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
        // Intentionally skip legacy rtfs_code population; use explicit events instead
        updateGraphStats();
        addLogEntry('Graph updated with new data.');
    }

    // Eager RTFS caches
    const intentRtfsCache = new Map(); // intent_id -> rtfs_code
    const graphRtfsCache = new Map(); // graph_id -> rtfs_code
    
    // Format toggle state
    let isRtfsMode = true; // Default to RTFS mode

    function handleIntentRtfsGenerated(data) {
        if (!data || !data.intent_id) return;
        intentRtfsCache.set(data.intent_id, data.rtfs_code || '');
        // If this intent is selected and Intent tab is active, render it
        if (selectedIntentId === data.intent_id && isTabActive('intent')) {
            renderIntentRtfs(data.intent_id);
        }
    }

    function handleGraphRtfsGenerated(data) {
        if (!data || !data.graph_id) return;
        graphRtfsCache.set(data.graph_id, data.rtfs_code || '');
        if (currentGraphId === data.graph_id && isTabActive('graph')) {
            renderGraphRtfs(data.graph_id);
        }
    }

    function isTabActive(which) {
        if (which === 'intent') return tabIntent && tabIntent.classList.contains('active');
        if (which === 'plan') return tabPlan && tabPlan.classList.contains('active');
        if (which === 'graph') return tabGraph && tabGraph.classList.contains('active');
        return false;
    }

    function renderIntentRtfs(intentId) {
        if (!intentCodeElement) return;
        const node = intentNodes.get(intentId);
        if (!node) {
            console.log(`❌ Node not found for intentId: ${intentId}`);
            return;
        }
        
        console.log(`🎯 Rendering RTFS for intent: ${intentId}, mode: ${isRtfsMode ? 'RTFS' : 'JSON'}`);
        console.log(`📝 RTFS cache has:`, intentRtfsCache.has(intentId) ? intentRtfsCache.get(intentId) : 'NOT FOUND');
        
        let code = '';
        if (isRtfsMode) {
            code = intentRtfsCache.get(intentId) || '';
        } else {
            // JSON representation
            code = JSON.stringify({
                id: node.id,
                label: node.label,
                goal: node.goal,
                type: node.type,
                status: node.status,
                created_at: node.created_at,
                execution_order: node.execution_order,
                is_root: node.is_root,
                rtfs_intent_source: intentRtfsCache.get(intentId) || ''
            }, null, 2);
        }
        
        console.log(`📄 Final code to display (${code.length} chars):`, code.substring(0, 100) + (code.length > 100 ? '...' : ''));
        
        intentCodeElement.textContent = code;
        intentCodeElement.className = isRtfsMode ? 'language-lisp' : 'language-json';
        if (typeof Prism !== 'undefined') Prism.highlightElement(intentCodeElement);
    }

    function renderGraphRtfs(graphId) {
        if (!graphCodeElement) return;
        
        let code = '';
        if (isRtfsMode) {
            code = graphRtfsCache.get(graphId) || '';
        } else {
            // JSON representation of current graph
            const nodes = Array.from(intentNodes.values());
            const edges = Array.from(intentEdges.values());
            code = JSON.stringify({
                graph_id: graphId,
                nodes: nodes,
                edges: edges,
                metadata: {
                    node_count: nodes.length,
                    edge_count: edges.length,
                    generated_at: new Date().toISOString()
                }
            }, null, 2);
        }
        
        graphCodeElement.textContent = code;
        graphCodeElement.className = isRtfsMode ? 'language-lisp' : 'language-json';
        if (typeof Prism !== 'undefined') Prism.highlightElement(graphCodeElement);
    }

    function renderPlanRtfs(intentId) {
        if (!planCodeElement) return;
        const plan = generatedPlans.get(intentId);
        
        let code = '';
        if (isRtfsMode) {
            code = plan?.body || '';
        } else {
            // JSON representation
            code = JSON.stringify(plan || {
                intent_id: intentId,
                body: '',
                status: 'not_found',
                message: 'No plan available for this intent'
            }, null, 2);
        }
        
        planCodeElement.textContent = code;
        planCodeElement.className = isRtfsMode ? 'language-lisp' : 'language-json';
        if (typeof Prism !== 'undefined') Prism.highlightElement(planCodeElement);
    }

    function toggleFormat() {
        isRtfsMode = !isRtfsMode;
        if (toggleFormatBtn) {
            toggleFormatBtn.textContent = isRtfsMode ? 'RTFS' : 'JSON';
            toggleFormatBtn.className = `format-toggle ${isRtfsMode ? 'rtfs-mode' : 'json-mode'}`;
        }
        // Re-render current active tab content
        if (isTabActive('intent') && selectedIntentId) renderIntentRtfs(selectedIntentId);
        if (isTabActive('plan') && selectedIntentId) renderPlanRtfs(selectedIntentId);
        if (isTabActive('graph') && currentGraphId) renderGraphRtfs(currentGraphId);
    }

    function activateTab(which) {
        const panes = [
            { btn: tabIntent, pane: document.getElementById('pane-intent') },
            { btn: tabPlan, pane: document.getElementById('pane-plan') },
            { btn: tabGraph, pane: document.getElementById('pane-graph') },
        ];
        panes.forEach(({ btn, pane }) => {
            if (!btn || !pane) return;
            const active = (btn.id === `tab-${which}`);
            btn.classList.toggle('active', active);
            pane.classList.toggle('hidden', !active);
        });
        if (which === 'intent' && selectedIntentId) renderIntentRtfs(selectedIntentId);
        if (which === 'plan' && selectedIntentId) renderPlanRtfs(selectedIntentId);
        if (which === 'graph' && currentGraphId) renderGraphRtfs(currentGraphId);
    }

    if (tabIntent && tabPlan && tabGraph) {
        tabIntent.addEventListener('click', () => activateTab('intent'));
        tabPlan.addEventListener('click', () => activateTab('plan'));
        tabGraph.addEventListener('click', () => activateTab('graph'));
        // Keep initial active tab
        activateTab('intent');
    }

    if (toggleFormatBtn) {
        toggleFormatBtn.addEventListener('click', toggleFormat);
        // Initialize format toggle button
        toggleFormatBtn.textContent = isRtfsMode ? 'RTFS' : 'JSON';
        toggleFormatBtn.className = `format-toggle ${isRtfsMode ? 'rtfs-mode' : 'json-mode'}`;
    }

    // Keyboard shortcuts for tab switching
    document.addEventListener('keydown', (e) => {
        // Only trigger if not in an input field
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

        switch (e.key) {
            case '1':
                e.preventDefault();
                activateTab('intent');
                addLogEntry('info', '⌨️ Switched to Intent tab (1)');
                break;
            case '2':
                e.preventDefault();
                activateTab('plan');
                addLogEntry('info', '⌨️ Switched to Plan tab (2)');
                break;
            case '3':
                e.preventDefault();
                activateTab('graph');
                addLogEntry('info', '⌨️ Switched to Graph tab (3)');
                break;
            case 'c':
                if (e.ctrlKey) {
                    e.preventDefault();
                    copyToClipboard();
                    addLogEntry('info', '⌨️ Copied to clipboard (Ctrl+C)');
                }
                break;
            case 't':
                if (e.ctrlKey || e.metaKey) {
                    e.preventDefault();
                    toggleFormat();
                    addLogEntry('info', `⌨️ Toggled format to ${isRtfsMode ? 'RTFS' : 'JSON'} (Ctrl+T)`);
                }
                break;
        }
    });

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

    // Execution event handlers
    const executionRuns = new Map(); // execution_id -> run data
    const executionLog = []; // { ts, run, type, intent_id?, phase?, error?, result?, message }

    function renderExecutionRuns() {
        const listEl = document.getElementById('exec-runs-list');
        if (!listEl) return;
        listEl.innerHTML = '';
        const runs = Array.from(executionRuns.entries()).sort((a,b) => b[1].started_at - a[1].started_at);
        runs.forEach(([id, run]) => {
            const li = document.createElement('li');
            li.className = 'exec-run-item';
            const status = run.success === null ? 'running' : (run.success ? 'success' : 'partial');
            li.innerHTML = `<span class="exec-run-id" title="${id}">${id.substring(0,8)}</span>`+
                           `<span class="exec-run-status exec-status-${status}">${status}</span>`+
                           `<span class="exec-run-time">${timeAgo(run.started_at)}</span>`;
            li.addEventListener('click', () => renderExecutionDetails(id));
            listEl.appendChild(li);
        });
        // Auto-show bottom panel when there are runs
        if (typeof window.showExecutionPanel === 'function' && runs.length > 0) {
            window.showExecutionPanel();
        }
    }

    function timeAgo(tsSec) {
        const delta = Date.now()/1000 - tsSec;
        if (delta < 60) return `${Math.floor(delta)}s ago`;
        if (delta < 3600) return `${Math.floor(delta/60)}m ago`;
        return `${Math.floor(delta/3600)}h ago`;
    }

    let currentExecSubtab = 'intents';
    function setExecSubtab(tab){
        currentExecSubtab = tab;
        document.querySelectorAll('.exec-subtab').forEach(btn => {
            btn.classList.toggle('active', btn.dataset.subtab === tab);
        });
        document.querySelectorAll('#exec-subtab-panels .exec-subtab-panel').forEach(p => {
            if (!p.id) return;
            const is = p.id === `subtab-${tab}`;
            p.classList.toggle('hidden', !is);
        });
    }
    function buildIntentTimeline(run){
        const timelineEl = document.getElementById('exec-intent-timeline');
        if (!timelineEl) return;
        timelineEl.innerHTML = '';
        const entries = Object.entries(run.intents);
        entries.forEach(([intentId, info]) => {
            const chip = document.createElement('div');
            const phases = info.phases.map(p=>p.phase);
            const finalPhase = phases[phases.length-1] || '';
            const statusClass = finalPhase === 'failed' ? 'fail' : finalPhase === 'completed' ? 'success' : finalPhase === 'skipped' ? 'skipped' : (finalPhase ? 'running' : '');
            chip.className = `intent-chip ${statusClass}`;
            chip.title = `${intentId}\n${phases.join(' → ')}`;
            const dotsUnique = [];
            for (const ph of phases){ if (!dotsUnique.includes(ph)) dotsUnique.push(ph); }
            // Prefer a short human label when available, otherwise show the id prefix
            const node = intentNodes.get(intentId);
            const displayLabel = node?.label ? (node.label.length > 18 ? node.label.substring(0,18) + '…' : node.label) : intentId.substring(0,8);
            // Show label (or id prefix) and the id prefix in monospace for clarity
            chip.innerHTML = `<span class="ic-id">${escapeHtml(displayLabel)}</span><div style="font-family:monospace;font-size:11px;color:#999;margin-top:2px">${escapeHtml(intentId.substring(0,8))}</div><div class="ic-phases">${dotsUnique.map(ph=>`<span class='phase-dot ${ph}'></span>`).join('')}</div>`;
            chip.addEventListener('click', () => {
                try {
                    // Select the intent logically
                    selectIntent(intentId);
                    // Vis.js select in graph view
                    if (typeof network !== 'undefined' && network && typeof network.selectNodes === 'function') {
                        network.selectNodes([intentId], false);
                        // center on node for user context
                        try {
                            const pos = network.getPositions([intentId])[intentId];
                            if (pos) network.moveTo({ position: pos, scale: 1.2, animation: { duration: 300 } });
                        } catch (e) { /* ignore positioning errors */ }
                    }
                    // Switch to the Intent tab and render code
                    activateTab('intent');
                    if (typeof renderIntentRtfs === 'function') renderIntentRtfs(intentId);
                    // Also highlight the corresponding row in the exec table
                    highlightIntentRow(intentId);
                } catch (e) {
                    console.error('Error handling timeline chip click:', e);
                }
            });
            timelineEl.appendChild(chip);
        });
    }
    function highlightIntentRow(intentId){
        const row = document.querySelector(`.exec-intents-table tbody tr[data-intent='${intentId}']`);
        if (row){
            row.classList.add('highlight-intent-row');
            row.scrollIntoView({block:'nearest'});
            setTimeout(()=> row.classList.remove('highlight-intent-row'), 1500);
        }
        setExecSubtab('intents');
    }
    function renderExecIntentsTable(run){
        const container = document.getElementById('subtab-intents');
        if (!container) return;
        container.innerHTML = '';
        const wrapper = document.createElement('div');
        wrapper.className = 'exec-intents-wrapper';
        const table = document.createElement('table');
        table.className = 'exec-intents-table';
        table.innerHTML = `<thead><tr><th>Intent</th><th>Phases</th><th>Result</th><th>Error</th></tr></thead><tbody></tbody>`;
        const tbody = table.querySelector('tbody');
        Object.entries(run.intents).forEach(([intentId, info]) => {
            const tr = document.createElement('tr');
            tr.dataset.intent = intentId;
            const phases = info.phases.map(p=>p.phase);
            const uniqueOrdered = [];
            for (const ph of phases){ if(!uniqueOrdered.includes(ph)) uniqueOrdered.push(ph); }
            const phaseHtml = uniqueOrdered.map(ph=>`<span class='phase-mini ${ph}'>${ph[0].toUpperCase()}</span>`).join('');
            const resultPill = info.result ? `<span class='result-pill' title='${escapeHtml(JSON.stringify(info.result))}'>res</span>` : '';
            const errorPill = info.error ? `<span class='error-pill' title='${escapeHtml(info.error)}'>err</span>` : '';
            tr.innerHTML = `<td title='${intentId}'>${intentId.substring(0,8)}</td><td class='phase-seq'>${phaseHtml}</td><td>${resultPill}</td><td>${errorPill}</td>`;
            tbody.appendChild(tr);
        });
        wrapper.appendChild(table);
        container.appendChild(wrapper);
    }
    function renderExecLog(executionId){
        const execLogEl = document.getElementById('exec-log-entries');
        if (!execLogEl) return;
        execLogEl.innerHTML='';
        executionLog.filter(e=> e.run===executionId).forEach(e => {
            const div = document.createElement('div');
            const icon = (
                e.type === 'start' ? '🚀' :
                e.type === 'finish' ? (e.success ? '🏁' : '⚠️') :
                e.type === 'summary' ? '📊' :
                e.type === 'intent-phase' ? (e.error ? '❌' : '🧩') : '•'
            );
            const intentSpan = e.intent_id ? `<span class="exec-intent-id" title="${e.intent_id}">${e.intent_id.substring(0,6)}</span>` : '';
            const errorSpan = e.error ? `<span class="exec-error">${escapeHtml(e.error)}</span>` : '';
            const resultSpan = e.result ? `<span class="exec-result" title='${escapeHtml(JSON.stringify(e.result))}'>res</span>` : '';
            div.className = `exec-log-entry exec-type-${e.type} ${e.error ? 'has-error' : ''}`;
            div.innerHTML = `<span class='exec-log-time'>${new Date(e.ts).toLocaleTimeString()}</span><span class='exec-log-icon'>${icon}</span>${intentSpan}<span class='exec-log-message'>${e.message||''}</span>${resultSpan}${errorSpan}`;
            execLogEl.appendChild(div);
        });
        execLogEl.scrollTop = execLogEl.scrollHeight;
    }
    function renderExecutionDetails(executionId) {
        const run = executionRuns.get(executionId);
        if (!run) return;
        const titleEl = document.getElementById('exec-run-title');
        const statusEl = document.getElementById('exec-run-status');
        const metaEl = document.getElementById('exec-run-meta');
        const statsEl = document.getElementById('exec-run-stats');
        const summaryEl = document.getElementById('exec-summary-json');
        if (titleEl) titleEl.textContent = `Execution ${executionId.substring(0,8)}`;
        if (statusEl) {
            const status = run.success === null ? 'RUNNING' : (run.success ? 'SUCCESS' : 'PARTIAL/FAIL');
            statusEl.textContent = status;
            statusEl.className = 'exec-status-badge ' + (run.success === null ? 'exec-status-running' : (run.success ? 'exec-status-success' : 'exec-status-fail'));
        }
        if (metaEl) {
            metaEl.innerHTML = `Started: ${new Date(run.started_at*1000).toLocaleTimeString()}<br>`+
                               `Finished: ${run.finished_at ? new Date(run.finished_at*1000).toLocaleTimeString() : '—'}<br>`+
                               `Duration: ${run.finished_at ? (run.finished_at - run.started_at).toFixed(1)+'s' : '—'}`;
        }
        if (statsEl){
            let success=0, fail=0, skipped=0, total=0;
            Object.values(run.intents).forEach(info => {
                total++;
                const phases = info.phases.map(p=>p.phase);
                if (phases.includes('failed')) fail++; else if (phases.includes('completed')) success++; else if (phases.includes('skipped')) skipped++; });
            statsEl.innerHTML = `<span class='exec-stat-chip'>INTENTS ${total}</span><span class='exec-stat-chip success'>${success}</span><span class='exec-stat-chip fail'>${fail}</span><span class='exec-stat-chip skipped'>${skipped}</span>`;
        }
        buildIntentTimeline(run);
        renderExecIntentsTable(run);
        if (summaryEl) summaryEl.textContent = run.summary ? JSON.stringify(run.summary, null, 2) : 'No summary yet.';
        renderExecLog(executionId);
        setExecSubtab(currentExecSubtab);
    }

    // Subtab events (delegated after DOM ready)
    document.addEventListener('click', (e) => {
        const btn = e.target.closest('.exec-subtab');
        if (btn){
            setExecSubtab(btn.dataset.subtab);
        }
    });

    function escapeHtml(str){
        return str.replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;','\'':'&#39;'}[c]));
    }

    function handleExecutionStarted(data) {
        if (!data || !data.execution_id) return;
        executionRuns.set(data.execution_id, {
            started_at: data.started_at || Date.now()/1000,
            finished_at: null,
            success: null,
            intents: {}
        });
    executionLog.push({ ts: Date.now(), run: data.execution_id, type: 'start', message: 'Execution started' });
        addLogEntry(`[EXEC] Execution started (ID: ${data.execution_id.substring(0,8)})`);
    renderExecutionRuns();
    }

    function handleIntentExecution(data) {
        if (!data || !data.execution_id || !data.intent_id) return;
        const run = executionRuns.get(data.execution_id);
        if (!run) {
            // Late creation if started event missed
            executionRuns.set(data.execution_id, {
                started_at: data.occurred_at || Date.now()/1000,
                finished_at: null,
                success: null,
                intents: {}
            });
        }
        const exec = executionRuns.get(data.execution_id);
        if (!exec.intents[data.intent_id]) {
            exec.intents[data.intent_id] = { phases: [], result: null, error: null };
        }
        exec.intents[data.intent_id].phases.push({ phase: data.phase, at: data.occurred_at });
        if (data.result) exec.intents[data.intent_id].result = data.result;
        if (data.error) exec.intents[data.intent_id].error = data.error;
        // Add concise log line
        executionLog.push({
            ts: Date.now(),
            run: data.execution_id,
            type: 'intent-phase',
            intent_id: data.intent_id,
            phase: data.phase,
            error: data.error || null,
            result: data.result || null,
            message: `Intent phase update`
        });
        addLogEntry(`[EXEC] ${data.intent_id.substring(0,8)} phase: ${data.phase}` + (data.error ? ` ❌ ${data.error}` : '') + (data.result ? ` ✅ result` : ''));
    renderExecutionDetails(data.execution_id);
    }

    function handleExecutionFinished(data) {
        if (!data || !data.execution_id) return;
        const run = executionRuns.get(data.execution_id);
        if (run) {
            run.finished_at = data.finished_at || Date.now()/1000;
            run.success = data.success;
            run.summary = data.summary;
        }
    executionLog.push({ ts: Date.now(), run: data.execution_id, type: 'finish', success: data.success, message: 'Execution finished' });
        addLogEntry(`[EXEC] Execution finished (ID: ${data.execution_id.substring(0,8)}) status: ${data.success ? 'SUCCESS' : 'FAIL/PARTIAL'}`);
    if (data.summary) executionLog.push({ ts: Date.now(), run: data.execution_id, type: 'summary', summary: data.summary, message: 'Execution summary' });
        if (data.summary) addLogEntry(`[EXEC] Summary: ${JSON.stringify(data.summary)}`);
        renderExecutionRuns();
        renderExecutionDetails(data.execution_id);
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
            console.log('  - generatedPlans keys:', Array.from(generatedPlans.keys()));
            console.log('  - Edge details:', Array.from(intentEdges.entries()));
            console.log('  - Edge keys:', Array.from(intentEdges.keys()));
            console.log('  - Edge values:', Array.from(intentEdges.values()));
            
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
    async function restoreGraphFromHistory(graphId) {
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

        console.log('🔁 restoreGraphFromHistory: restored generatedPlans size =', generatedPlans.size);
        console.log('🔁 restoreGraphFromHistory: generatedPlans keys =', Array.from(generatedPlans.keys()));

        
        console.log('📊 Restoring graph data:');
        console.log('  - Nodes:', intentNodes.size);
        console.log('  - Edges:', intentEdges.size);
        console.log('  - Plans:', generatedPlans.size);
        console.log('  - Root ID:', currentGraphId);
        console.log('  - Historical edges:', Array.from(intentEdges.entries()));

        // Inform server to rehydrate this graph into CCOS so that plan generation works
        let rehydrationSuccess = false;
        try {
            const nodesToSend = Array.from(intentNodes.values());
            const edgesToSend = Array.from(intentEdges.values());
            console.log('🔄 Rehydrating graph on server:');
            console.log('  - Nodes to send:', nodesToSend.length);
            console.log('  - Edges to send:', edgesToSend.length);
            console.log('  - Edge details:', edgesToSend);
            console.log('  - Root ID:', currentGraphId);
            
            const rehydrateResp = await fetch('/load-graph', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    nodes: nodesToSend,
                    edges: edgesToSend,
                    root_id: currentGraphId
                })
            });
            if (rehydrateResp.ok) {
                const r = await rehydrateResp.json();
                if (r.success) {
                    console.log(`✅ Server rehydrated graph ${r.graph_id} into CCOS`);
                    rehydrationSuccess = true;
                } else {
                    console.warn('⚠️ Server failed to rehydrate graph:', r.error);
                    // Graph doesn't exist on server - remove from history and show message
                    addLogEntry(`❌ Graph "${historicalGraph.name}" not found on server. Removing from history.`);
                    graphHistory.delete(graphId);
                        populateGraphHistorySelector();
                    addLogEntry(`🗑️ Removed invalid graph from history. ${graphHistory.size} graphs remaining.`);
                    return false; // Indicate failure
                }
            } else {
                console.warn('⚠️ /load-graph returned non-OK');
                // Server error - remove from history and show message
                addLogEntry(`❌ Server error loading graph "${historicalGraph.name}". Removing from history.`);
                graphHistory.delete(graphId);
                        populateGraphHistorySelector();
                addLogEntry(`🗑️ Removed invalid graph from history. ${graphHistory.size} graphs remaining.`);
                return false; // Indicate failure
            }
        } catch (e) {
            console.warn('⚠️ Error rehydrating graph on server:', e);
            // Network error - remove from history and show message
            addLogEntry(`❌ Network error loading graph "${historicalGraph.name}". Removing from history.`);
            graphHistory.delete(graphId);
                        populateGraphHistorySelector();
            addLogEntry(`🗑️ Removed invalid graph from history. ${graphHistory.size} graphs remaining.`);
            return false; // Indicate failure
        }

        // Only proceed with graph restoration if rehydration was successful
        if (!rehydrationSuccess) {
            return false;
        }

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
                const hasPlan = generatedPlans.has(nodeId);
                const labelText = node.label || nodeId;
                const planIndicator = hasPlan ? ' 📋' : '';
                
                nodeData = {
                    id: nodeId,
                    label: labelText + planIndicator,
                    level: 0, // Force root node to be at the top level
                    color: {
                        border: hasPlan ? '#00ff88' : '#FFD700', // Green if has plan, gold otherwise
                        background: '#2a2a2a',
                        highlight: { border: hasPlan ? '#00ff88' : '#FFD700', background: '#3a3a3a' }
                    },
                    title: `${baseTitle}\n\n🎯 Root Intent - Orchestrates execution of child intents${getBadgeText(nodeId)}`,
                    shape: 'diamond',
                    size: 30,
                    borderWidth: hasPlan ? 3 : 2,
                    font: { size: 16, color: hasPlan ? '#00ff88' : '#FFD700', face: 'arial' }
                };
            } else {
                // Child nodes: depth-based level with execution order in label
                const depth = nodeDepths.get(nodeId) || 1;
                const hasPlan = generatedPlans.has(nodeId);
                const labelText = node.label || nodeId;
                const planIndicator = hasPlan ? ' 📋' : '';
                
                nodeData = {
                    id: nodeId,
                    label: labelText + planIndicator,
                    level: depth, // Use depth-based level for proper tree visualization
                    color: hasPlan ? {
                        border: '#00ff88',
                        background: '#2a2a2a',
                        highlight: { border: '#00ff88', background: '#3a3a3a' }
                    } : getNodeColor(node.status || 'pending'),
                    borderWidth: hasPlan ? 3 : 2,
                    title: `${baseTitle}\nExecution Order: ${node.execution_order || 'N/A'}\nDepth Level: ${depth}\n\n💡 Same depth = same execution level, numbers show sequence${getBadgeText(nodeId)}`
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
            // Ensure code panes are sized after restoring graph
            try { updateCodePaneHeights(); } catch (e) { console.warn('⚠️ updateCodePaneHeights failed after restore:', e); }
            
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
                        console.log('📋 plansResult.plans:', plansResult.plans.map(p=>p.intent_id));
                        
                        // Store the retrieved plans
                        for (const plan of plansResult.plans) {
                            generatedPlans.set(plan.intent_id, {
                                plan_id: plan.plan_id,
                                body: plan.body,
                                intent_id: plan.intent_id,
                                status: plan.status
                            });
                            
                            // Update node visual styling to show it has a plan
                            updateNodePlanIndicator(plan.intent_id, true);
                        }
                        
                        // Update UI to show plan indicators on nodes
                        console.log('🔄 Updating node UI to show plan indicators...');
                        console.log('🔄 After storing retrieved plans, generatedPlans size =', generatedPlans.size);
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
                        
                        // Sync graphHistory entry as well
                        if (graphHistory.has(currentGraphId)) {
                            const gh = graphHistory.get(currentGraphId);
                            console.log('🔁 restoreGraphFromHistory: syncing graphHistory plans. gh before size =', gh.plans.size);
                            gh.plans = new Map(generatedPlans);
                            graphHistory.set(currentGraphId, gh);
                            console.log('🔁 restoreGraphFromHistory: gh after size =', gh.plans.size);
                            populateGraphHistorySelector();
                        }

                        addLogEntry(`📋 Retrieved ${plansResult.plans.length} plans from server`);
                    } else {
                        console.log('📋 No plans found on server for this graph');
                        addLogEntry('📋 No plans found on server for this graph. Use "Generate Plans" button to create them.');
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
        
        // Update status bar and hide activity
        updateStatusBar();
        hideActivity();
        addLogEntry('info', `✅ Graph generated: ${data.graph_id}`);
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
        // Note: intentEdges.clear() removed - edges will be populated asynchronously later
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
                    const hasPlan = generatedPlans.has(node.id);
                    const labelText = node.label || node.id;
                    const planIndicator = hasPlan ? ' 📋' : '';
                    
                    nodeData = {
                        id: node.id,
                        label: labelText + planIndicator,
                        level: 0, // Force root node to be at the top level
                        color: {
                            border: hasPlan ? '#00ff88' : '#FFD700', // Green if has plan, gold otherwise
                            background: '#2a2a2a',
                            highlight: { border: hasPlan ? '#00ff88' : '#FFD700', background: '#3a3a3a' }
                        },
                        title: `${baseTitle}\n\n🎯 Root Intent - Orchestrates execution of child intents${getBadgeText(node.id)}`,
                        shape: 'diamond',
                        size: 30,
                        borderWidth: hasPlan ? 3 : 2,
                        font: { size: 16, color: hasPlan ? '#00ff88' : '#FFD700', face: 'arial' }
                    };
                } else {
                    // Child nodes: depth-based level with execution order in label
                    const depth = nodeDepths.get(node.id) || 1;
                    const hasPlan = generatedPlans.has(node.id);
                    const labelText = node.label || node.id;
                    const planIndicator = hasPlan ? ' 📋' : '';
                    
                    nodeData = {
                        id: node.id,
                        label: labelText + planIndicator,
                        level: depth, // Use depth-based level for proper tree visualization
                        color: hasPlan ? {
                            border: '#00ff88',
                            background: '#2a2a2a',
                            highlight: { border: '#00ff88', background: '#3a3a3a' }
                        } : getNodeColor(node.status || 'pending'),
                        borderWidth: hasPlan ? 3 : 2,
                        title: `${baseTitle}\nExecution Order: ${node.execution_order || 'N/A'}\nDepth Level: ${depth}\n\n💡 Same depth = same execution level, numbers show sequence${getBadgeText(node.id)}`
                    };

                    // Add visual emphasis for execution order
                    if (node.execution_order) {
                        const baseLabel = node.label || node.id;
                        const cleanLabel = baseLabel.replace(/^\d+\.\s*/, '');
                        nodeData.label = `🔢 ${node.execution_order}. ${cleanLabel}${planIndicator}`;
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
                        
                        // Extract RTFS from rtfs_intent_source field if present
                        if (node.rtfs_intent_source) {
                            intentRtfsCache.set(node.id, node.rtfs_intent_source);
                            console.log(`📝 Extracted RTFS for updated node ${node.id}:`, node.rtfs_intent_source);
                        }
                        
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
                    
                    // Extract RTFS from rtfs_intent_source field if present
                    if (node.rtfs_intent_source) {
                        intentRtfsCache.set(node.id, node.rtfs_intent_source);
                        console.log(`📝 Extracted RTFS for node ${node.id}:`, node.rtfs_intent_source);
                    }
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
                    console.log(`🔍 intentEdges Map now has ${intentEdges.size} edges`);
                    console.log(`🔍 intentEdges contents:`, Array.from(intentEdges.keys()));
                } catch (error) {
                    console.error(`❌ Failed to add edge ${edgeId}:`, error);
                    console.error('Edge data:', edgeData);
                    return; // Skip this edge
                }
            });
            }, 50); // 50ms delay to ensure clearing is complete
        }

        // Store the new graph in history after edge processing is complete
        setTimeout(() => {
            console.log('💾 Storing new graph in history after edge processing...');
            storeCurrentGraphInHistory();
        }, 100); // Store after edges are processed

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
        // Note: This will be called after edge processing is complete
    }

    function handlePlanGenerated(data) {
        // Only process plans for the current graph
        if (currentGraphId && data.graph_id !== currentGraphId) {
            console.log(`📊 Ignoring plan for graph ${data.graph_id}, current graph is ${currentGraphId}`);
            return;
        }

        executeBtn.disabled = false;
        updateGoalStatus('Plans generated successfully. Ready to execute.');
        addLogEntry('info', `✅ Plan generated for intent ${data.intent_id}: ${data.plan_id}`);
        
        // Hide activity indicator when first plan is generated
        hideActivity();

        // Store plan information for later retrieval
        generatedPlans.set(data.intent_id, {
            plan_id: data.plan_id,
            body: data.rtfs_code,
            intent_id: data.intent_id,
            graph_id: data.graph_id,
            timestamp: new Date().toISOString()
        });

        console.log('📋 handlePlanGenerated: generatedPlans size =', generatedPlans.size);
        console.log('📋 handlePlanGenerated: generatedPlans keys =', Array.from(generatedPlans.keys()));

        // Update node visual styling to show it has a plan
        updateNodePlanIndicator(data.intent_id, true);

        // Keep graph history in sync so plan counts in the selector update
        try {
            if (currentGraphId && graphHistory.has(currentGraphId)) {
                const gh = graphHistory.get(currentGraphId);
                console.log('🔁 handlePlanGenerated: gh before size =', gh.plans.size);
                gh.plans = new Map(generatedPlans);
                graphHistory.set(currentGraphId, gh);
                console.log('🔁 handlePlanGenerated: gh after size =', gh.plans.size);
                populateGraphHistorySelector();
            }
        } catch (e) {
            console.warn('⚠️ Failed to sync graphHistory after plan generation:', e);
        }

        // Update plan pane if active
        if (isTabActive('plan') && selectedIntentId === data.intent_id) {
            renderPlanRtfs(data.intent_id);
        }

        console.log(`📋 Stored plan for intent ${data.intent_id}:`, generatedPlans.get(data.intent_id));
    }

    function handleReadyForNext(data) {
        updateGoalStatus(`Ready for next step: ${data.next_step}`);
        addLogEntry(`Ready for next step: ${data.next_step}`);
    }

    // Legacy plan details function (kept for compatibility but tabs take precedence)
    function showPlanDetails(node) {
        // Prefer tabs-based rendering
        selectedIntentId = node.id;
        activateTab('plan');
        renderPlanRtfs(node.id);
        return;

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

        const planCodeText = storedPlan ? storedPlan.body : (node.plan_body_preview || 'Plan code not available');

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

        addLogEntry(`📋 Displaying plan: ${node.original_label || node.label}`);
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
            case 'pending_execution':
                return {
                    border: '#555555',
                    background: '#2a2a2a',
                    highlight: { border: '#777777', background: '#333333' }
                };
            case 'skipped':
                return {
                    border: '#888888',
                    background: '#2f2f2f',
                    highlight: { border: '#aaaaaa', background: '#3a3a3a' }
                };
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
