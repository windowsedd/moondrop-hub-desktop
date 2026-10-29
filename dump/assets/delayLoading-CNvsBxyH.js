function o(e=200){return async(t,r)=>{t.controlLoading();const{loading:a}=t.proxyStates,n=setTimeout(()=>{a.v=!0},e);try{return await r()}finally{clearTimeout(n),a.v=!1}}}export{o as d};
