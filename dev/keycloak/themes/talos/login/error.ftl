<#import "template.ftl" as layout>
<@layout.registrationLayout displayMessage=false; section>
    <#if section = "header">
        ${msg("errorTitle")}
    <#elseif section = "form">
        <div class="alert alert-error" role="alert">
            <span class="alert-tag">ERROR</span>
            <span class="alert-text">${kcSanitize(message.summary)?no_esc}</span>
        </div>
        <#if client?? && client.baseUrl?has_content>
            <p class="register-hint">
                <a id="backToApplication" href="${client.baseUrl}" class="linkish">${kcSanitize(msg("backToApplication"))?no_esc}</a>
            </p>
        <#else>
            <p class="register-hint">
                <a href="${url.loginUrl}" class="linkish">${msg("backToLogin")}</a>
            </p>
        </#if>
    </#if>
</@layout.registrationLayout>
