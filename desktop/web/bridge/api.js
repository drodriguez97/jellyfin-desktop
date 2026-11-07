// window.api shim: recreates the QWebChannel object surface that the
// nativeshell/mpv plugin scripts use (api.player.*, api.system.*, ...), on top
// of Tauri's invoke()/listen(). Methods return promises; the trailing callback
// argument that QWebChannel supported is still honoured for compatibility.
//
// Native -> web "signals" are objects with connect()/disconnect(), fed by Tauri
// events named "<object>:<signal>" whose payload is the argument list.
(function () {
    // Resolved lazily: Tauri's own globals may be injected after this script.
    const tauri = () => {
        const t = window.__TAURI__;
        if (!t) throw new Error('Tauri globals missing on ' + location.origin);
        return t;
    };
    const invoke = (cmd, args) => tauri().core.invoke(cmd, args);
    const listen = (name, handler) => tauri().event.listen(name, handler);

    function makeSignal(objectName, signalName) {
        const handlers = new Set();
        let unlisten = null;
        const eventName = objectName + ':' + signalName;
        const ensureListening = () => {
            if (unlisten) return;
            unlisten = true;
            listen(eventName, (event) => {
                // Payloads are emitted as argument tuples; a unit payload arrives as null.
                const p = event.payload;
                const args = Array.isArray(p) ? p : (p === null || p === undefined ? [] : [p]);
                for (const fn of Array.from(handlers)) {
                    try { fn(...args); } catch (e) { console.error('[api] handler for ' + eventName + ' failed:', e); }
                }
            }).catch((e) => { unlisten = null; console.error('[api] listen ' + eventName + ' failed:', e); });
        };
        return {
            connect(fn) { handlers.add(fn); ensureListening(); },
            disconnect(fn) { handlers.delete(fn); }
        };
    }

    // Converts positional call arguments into the named-argument object a
    // Tauri command expects. `params` lists the parameter names in order.
    function makeMethod(command, params) {
        return function (...args) {
            let callback = null;
            if (args.length > params.length && typeof args[args.length - 1] === 'function') {
                callback = args.pop();
            }
            const payload = {};
            params.forEach((name, i) => { if (i < args.length) payload[name] = args[i]; });
            const p = invoke(command, payload);
            if (callback) p.then(callback, (e) => { console.error('[api] ' + command + ' failed:', e); callback(undefined); });
            return p;
        };
    }

    function makeObject(name, methods, signals) {
        const obj = {};
        for (const [method, [command, params]] of Object.entries(methods)) {
            obj[method] = makeMethod(command, params);
        }
        for (const signal of signals) {
            obj[signal] = makeSignal(name, signal);
        }
        return obj;
    }

    const api = {
        player: makeObject('player', {
            load: ['player_load', ['url', 'options', 'metadata', 'audioStream', 'subtitleStream']],
            queueMedia: ['player_queue_media', ['url', 'options', 'metadata', 'audioStream', 'subtitleStream']],
            clearQueue: ['player_clear_queue', []],
            play: ['player_play', []],
            pause: ['player_pause', []],
            stop: ['player_stop', []],
            seekTo: ['player_seek_to', ['ms']],
            streamSwitch: ['player_stream_switch', []],
            getPosition: ['player_get_position', []],
            getDuration: ['player_get_duration', []],
            setVolume: ['player_set_volume', ['volume']],
            volume: ['player_volume', []],
            setMuted: ['player_set_muted', ['muted']],
            muted: ['player_muted', []],
            setPlaybackRate: ['player_set_playback_rate', ['rate']],
            setAudioStream: ['player_set_audio_stream', ['audioStream']],
            setSubtitleStream: ['player_set_subtitle_stream', ['subtitleStream']],
            setAudioDelay: ['player_set_audio_delay', ['milliseconds']],
            setSubtitleDelay: ['player_set_subtitle_delay', ['milliseconds']],
            setVideoRectangle: ['player_set_video_rectangle', ['x', 'y', 'w', 'h']],
            setVideoOnlyMode: ['player_set_video_only_mode', ['enable']],
            getAudioDeviceList: ['player_get_audio_device_list', []],
            setAudioDevice: ['player_set_audio_device', ['name']],
            userCommand: ['player_user_command', ['command']],
            setWebPlaylist: ['player_set_web_playlist', ['playlist', 'currentItemId']],
            getWebPlaylist: ['player_get_web_playlist', []],
            getCurrentWebPlaylistItemId: ['player_get_current_web_playlist_item_id', []],
            notifyShuffleChange: ['player_notify_shuffle_change', ['enabled']],
            notifyRepeatChange: ['player_notify_repeat_change', ['mode']],
            notifyFullscreenChange: ['player_notify_fullscreen_change', ['isFullscreen']],
            notifyRateChange: ['player_notify_rate_change', ['rate']],
            notifyQueueChange: ['player_notify_queue_change', ['canNext', 'canPrevious']],
            notifyPlaybackStop: ['player_notify_playback_stop', ['isNavigating']],
            notifyDurationChange: ['player_notify_duration_change', ['durationMs']],
            notifyPlaybackState: ['player_notify_playback_state', ['state']],
            notifyPosition: ['player_notify_position', ['positionMs']],
            notifySeek: ['player_notify_seek', ['positionMs']],
            notifyMetadata: ['player_notify_metadata', ['metadata']],
            notifyVolumeChange: ['player_notify_volume_change', ['volume']]
        }, [
            'playing', 'buffering', 'paused', 'finished', 'canceled', 'error', 'stopped', 'stateChanged',
            'videoPlaybackActive', 'windowVisible', 'updateDuration', 'playbackRateChanged', 'positionUpdate',
            'bufferedRangesUpdated', 'onVideoRecangleChanged', 'onMetaData', 'webPlaylistChanged',
            'shuffleChanged', 'repeatChanged', 'fullscreenChanged', 'rateChanged', 'queueChanged',
            'playbackStopped', 'durationChanged', 'playbackStateChanged', 'positionChanged', 'seekPerformed',
            'metadataChanged', 'volumeChanged'
        ]),
        system: makeObject('system', {
            exit: ['system_exit', []],
            restart: ['system_restart', []],
            hello: ['system_hello', ['version']],
            openExternalUrl: ['system_open_external_url', ['url']],
            checkServerConnectivity: ['system_check_server_connectivity', ['url']],
            cancelServerConnectivity: ['system_cancel_server_connectivity', []],
            checkForUpdates: ['system_check_for_updates', []],
            getCapabilitiesString: ['system_get_capabilities_string', []],
            networkAddresses: ['system_network_addresses', []],
            runUserScript: ['system_run_user_script', ['script']],
            // Kept for compatibility: with initialization scripts there is no
            // CSP problem, so this just reports "no CSP" for the given url.
            fetchPageForCSPWorkaround: ['system_fetch_page_for_csp_workaround', ['url']]
        }, [
            'serverConnectivityResult', 'serverConnectivityAttemptFailed', 'pageContentReady',
            'updateInfoEmitted', 'capabilitiesChanged', 'hostMessage', 'settingsMessage'
        ]),
        settings: makeObject('settings', {
            setValue: ['settings_set_value', ['section', 'key', 'value']],
            setValues: ['settings_set_values', ['options']],
            value: ['settings_value', ['section', 'key']],
            allValues: ['settings_all_values', ['section']],
            removeValue: ['settings_remove_value', ['sectionOrKey']],
            resetToDefault: ['settings_reset_to_default', ['section']],
            resetToDefaultAll: ['settings_reset_to_default', []],
            settingDescriptions: ['settings_descriptions', []],
            orderedSections: ['settings_ordered_sections', []]
        }, ['sectionValueUpdate', 'groupUpdate']),
        input: makeObject('input', {
            executeActions: ['input_execute_actions', ['actions']],
            setVolume: ['input_set_volume', ['volume']],
            seekTo: ['input_seek_to', ['position']],
            setRate: ['input_set_rate', ['rate']]
        }, ['hostInput', 'receivedInput', 'volumeChanged', 'positionSeek', 'rateChanged']),
        window: makeObject('window', {
            setFullScreen: ['window_set_fullscreen', ['enable']],
            isFullScreen: ['window_is_fullscreen', []],
            toggleFullscreen: ['window_toggle_fullscreen', []],
            setAlwaysOnTop: ['window_set_always_on_top', ['enable']],
            isAlwaysOnTop: ['window_is_always_on_top', []],
            toggleAlwaysOnTop: ['window_toggle_always_on_top', []],
            setCursorVisibility: ['window_set_cursor_visibility', ['visible']],
            raiseWindow: ['window_raise', []]
        }, ['fullScreenSwitched']),
        power: makeObject('power', {}, ['screenSaverEnabled', 'screenSaverDisabled'])
    };

    window.api = api;
    window.apiPromise = Promise.resolve(api);
})();
