(window.webpackJsonp=window.webpackJsonp||[]).push([[5],{2:function(e,t,n){"use strict";n.r(t);var o=n(4);window.wasm=o;const a=o.bind_to("wrapper"),c=document.getElementById("wrapper");function d(e){const t="IMG"===e.target.nodeName?e.target:e.target.children[0];a.insert_image(t.src,t.alt,"emoji")}document.addEventListener("selectionchange",e=>{console.log("selectionchange",e),a.update_caret_position()}),c.addEventListener("keydown",e=>{if(console.log("keydown:",e),!e.ctrlKey&&!e.altKey&&!e.metaKey){a.process_key(e.key)&&e.preventDefault()}}),c.addEventListener("cut",e=>{console.log("cut",e),a.remove_selection(!1)}),c.addEventListener("paste",e=>{console.log("paste",e);const t=e.clipboardData.getData("text/plain");t&&(a.insert_text(t),e.preventDefault())}),document.getElementById("tongue").addEventListener("click",d),document.getElementById("beers").addEventListener("click",d),document.getElementById("facepalm").addEventListener("click",d)}}]);