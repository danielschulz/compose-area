(window["webpackJsonp"] = window["webpackJsonp"] || []).push([[5],{

/***/ 2:
/***/ (function(module, __webpack_exports__, __webpack_require__) {

"use strict";
__webpack_require__.r(__webpack_exports__);
/* harmony import */ var compose_area__WEBPACK_IMPORTED_MODULE_0__ = __webpack_require__(4);


// Assign wasm module to window object for testing purposes.
window.wasm = compose_area__WEBPACK_IMPORTED_MODULE_0__;

// Initialize compose area
const composeArea = compose_area__WEBPACK_IMPORTED_MODULE_0__["bind_to"]('wrapper');
window.composeArea = composeArea;

// Elements
const wrapper = document.getElementById('wrapper');
const logDiv = document.querySelector('#log div');
const extractedDiv = document.querySelector('#extracted div');
const selectionDiv = document.querySelector('#selection div');
const rawDiv = document.querySelector('#raw div');

// Helper functions

let startTime = null;

function log() {
    if (startTime === null) {
        startTime = new Date();
    }
    console.log(...arguments);
    const ms = (new Date() - startTime).toString();
    const pad = '      ';
    const timestamp = `${pad.substring(0, pad.length - ms.length) + ms}`;
    logDiv.innerHTML += `${timestamp} ${arguments[0]}<br>`;
}

function updateSelectionRange(e) {
    log('⚙️ store_selection_range');
    composeArea.store_selection_range();
    showState();
}

function showState() {
    // Extract text
    const text = composeArea.get_text();
    extractedDiv.innerText = text.replace(/\n/g, '↵\n');

    // Get range
    const range_result = composeArea.fetch_range();
    console.log(range_result);
    selectionDiv.innerText = range_result.to_string();

    // Get raw HTML
    rawDiv.innerText = wrapper.innerHTML;
}


// Add event listeners

wrapper.addEventListener('keydown', (e) => {
    log('⚡ keydown', e);
});
wrapper.addEventListener('keyup', (e) => {
    log('⚡ keyup', e);
});
wrapper.addEventListener('mouseup', (e) => {
    log('⚡ mouseup', e);
});
wrapper.addEventListener('paste', (e) => {
    log('⚡ paste', e);
    e.preventDefault();
    const clipboardData = e.clipboardData.getData('text/plain');
    if (clipboardData) {
        log('⚙️ insert_text');
        composeArea.insert_text(clipboardData);
    }
});

// Note: Unfortunately the selectionchange listener can only be set on document
// level, not on the wrapper itself.
document.addEventListener('selectionchange', (e) => {
    log('⚡ selectionchange', e);
    updateSelectionRange();
});

// Emoji handling

function insertEmoji(e) {
    const img = e.target.nodeName === 'IMG' ? e.target : e.target.children[0];
    log(`⚙️ insert_image`);
    composeArea.insert_image(img.src, img.alt, 'emoji');
    showState();
}
document.getElementById('tongue').addEventListener('click', insertEmoji);
document.getElementById('beers').addEventListener('click', insertEmoji);
document.getElementById('facepalm').addEventListener('click', insertEmoji);


/***/ })

}]);
//# sourceMappingURL=bootstrap.5.bundle.js.map