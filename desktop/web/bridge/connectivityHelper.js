window.jmpCheckServerConnectivity = (() => {
    let activeController = null;

    const checkFunc = async function(url, onAttemptFailed) {
        // Abort any in-progress check
        if (activeController) {
            activeController.abort();
        }

        // Wait for API
        let attempts = 0;
        while (!window.api && attempts < 50) {
            await new Promise(resolve => setTimeout(resolve, 100));
            attempts++;
        }
        if (!window.api) {
            throw new Error('WebChannel not available');
        }

        // Create abort controller for this check
        const controller = new AbortController();
        activeController = controller;

        return new Promise((resolve, reject) => {
            // Handle abort
            let failHandler = null;
            const disconnectFail = () => {
                if (failHandler && window.api.system.serverConnectivityAttemptFailed) {
                    window.api.system.serverConnectivityAttemptFailed.disconnect(failHandler);
                }
                failHandler = null;
            };

            controller.signal.addEventListener('abort', () => {
                if (handler) {
                    window.api.system.serverConnectivityResult.disconnect(handler);
                }
                disconnectFail();
                reject(new Error('Connection cancelled'));
            });

            let handler = (resultUrl, success, resolvedUrl) => {
                if (resultUrl === url && !controller.signal.aborted) {
                    window.api.system.serverConnectivityResult.disconnect(handler);
                    handler = null;
                    disconnectFail();
                    if (activeController === controller) {
                        activeController = null;
                    }
                    if (success) {
                        resolve(resolvedUrl);
                    } else {
                        reject(new Error('Connection failed'));
                    }
                }
            };

            if (onAttemptFailed && window.api.system.serverConnectivityAttemptFailed) {
                failHandler = (failedUrl, error) => {
                    if (failedUrl === url && !controller.signal.aborted) {
                        onAttemptFailed(error);
                    }
                };
                window.api.system.serverConnectivityAttemptFailed.connect(failHandler);
            }
            window.api.system.serverConnectivityResult.connect(handler);
            window.api.system.checkServerConnectivity(url);
        });
    };

    // Expose abort function for cancellation
    checkFunc.abort = () => {
        if (activeController) {
            activeController.abort();
            activeController = null;
        }
    };

    return checkFunc;
})();

window.jmpFetchPage = (() => {
    let fetchInProgress = false;

    return async function(url) {
        if (fetchInProgress) {
            throw new Error('Page fetch already in progress');
        }

        // Wait for API
        let attempts = 0;
        while (!window.api && attempts < 50) {
            await new Promise(resolve => setTimeout(resolve, 100));
            attempts++;
        }
        if (!window.api) {
            throw new Error('WebChannel not available');
        }

        fetchInProgress = true;

        return new Promise((resolve, reject) => {
            const handler = (html, finalUrl, hadCSP) => {
                window.api.system.pageContentReady.disconnect(handler);
                fetchInProgress = false;
                resolve({ html, finalUrl, hadCSP });
            };
            window.api.system.pageContentReady.connect(handler);
            window.api.system.fetchPageForCSPWorkaround(url);
        });
    };
})();
