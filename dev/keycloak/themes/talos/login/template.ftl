<#macro registrationLayout bodyClass="" displayInfo=false displayMessage=true displayRequiredFields=false>
<!DOCTYPE html>
<html lang="${locale.currentLanguageTag!'en'}" class="talos-auth">
<head>
    <meta charset="utf-8">
    <meta http-equiv="Content-Type" content="text/html; charset=UTF-8" />
    <meta name="robots" content="noindex, nofollow">
    <meta name="viewport" content="width=device-width,initial-scale=1"/>
    <title>${msg("loginTitle",(realm.displayName!''))}</title>
    <link rel="icon" href="${url.resourcesPath}/img/favicon.png" type="image/png"/>
    <link rel="preconnect" href="https://fonts.googleapis.com"/>
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin/>
    <link href="https://fonts.googleapis.com/css2?family=Fira+Code:wght@300;500;700&display=swap" rel="stylesheet"/>
    <#if properties.styles?has_content>
        <#list properties.styles?split(' ') as style>
            <link href="${url.resourcesPath}/${style}" rel="stylesheet"/>
        </#list>
    </#if>
</head>
<body class="talos-body ${bodyClass}">
    <div class="crt-overlay" aria-hidden="true"></div>
    <div class="talos-shell">
        <header class="talos-topbar">
            <div class="brand">
                <span class="pulse"></span>
                <span class="brand-text">TALOS // IDENTITY</span>
            </div>
            <div class="top-meta">SECURE CHANNEL</div>
        </header>

        <main class="talos-main">
            <section class="talos-card">
                <div class="card-head">
                    <img class="logo" src="${url.resourcesPath}/img/logo.png" alt="TALOS" width="40" height="40"/>
                    <h1><#nested "header"></h1>
                    <#if realm.displayName?has_content>
                        <p class="lede">${realm.displayName}</p>
                    <#else>
                        <p class="lede">VAULT-OS ACCESS GATE</p>
                    </#if>
                </div>

                <div class="card-body">
                    <#if displayMessage && message?has_content && (message.type != 'warning' || !isAppInitiatedAction??)>
                        <div class="alert alert-${message.type}" role="alert">
                            <span class="alert-tag">${message.type?upper_case}</span>
                            <span class="alert-text">${kcSanitize(message.summary)?no_esc}</span>
                        </div>
                    </#if>

                    <#nested "form">

                    <#if auth?has_content && auth.showTryAnotherWayLink()>
                        <form id="kc-select-try-another-way-form" action="${url.loginAction}" method="post" class="alt-form">
                            <input type="hidden" name="tryAnotherWay" value="on"/>
                            <button type="submit" class="linkish">${msg("doTryAnotherWay")}</button>
                        </form>
                    </#if>

                    <#if displayInfo>
                        <div class="info-block">
                            <#nested "info">
                        </div>
                    </#if>
                </div>

                <footer class="card-foot">
                    <span>NODE // KEYCLOAK</span>
                    <span>LAYER 0</span>
                </footer>
            </section>
        </main>
    </div>
</body>
</html>
</#macro>
