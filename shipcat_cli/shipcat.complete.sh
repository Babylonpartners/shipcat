#!/usr/bin/env bash

# shipcat(1) completion
_shipcat()
{
    # shellcheck disable=2034
    local cur prev words cword
    if declare -F _init_completions >/dev/null 2>&1; then
        _init_completion
    else
        # Mac support
        # Mac users also need bash and bash-completion installed from brew
        # They also need to source the line from the bash-completion package
        _get_comp_words_by_ref cur prev words cword
    fi

    local -r subcommands="help validate shell port-forward get graph cluster gdpr
                          kong debug list-regions list-services
                          apply template values status config crd diff login version"

    local has_sub
    for (( i=0; i < ${#words[@]}-1; i++ )); do
        if [[ ${words[i]} == @(help|validate|port-forward|diff|debug|get|config|version|status|statuscake|shell|graph|cluster|gdpr|kong|list-services|apply|template|values|status|crd) ]]; then
            has_sub=1
        fi
    done

    # global flags
    if [[ $prev = 'shipcat' && "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W '-v -h -V --version --help' -- "$cur" ) )
        return 0
    fi
    # first subcommand
    if [[ -z "$has_sub" ]]; then
        COMPREPLY=( $(compgen -W "$subcommands" -- "$cur" ) )
        return 0
    fi

    # special subcommand completions
    local special i
    for (( i=0; i < ${#words[@]}-1; i++ )); do
        if [[ ${words[i]} == @(list-services|validate|version|config|shell|port-forward|diff|debug|graph|get|cluster|gdpr|apply|template|values|status|crd) ]]; then
            special=${words[i]}
            break
        fi
    done

    local mdir="."
    if [ -n "$SHIPCAT_MANIFEST_DIR" ]; then
        mdir="${SHIPCAT_MANIFEST_DIR}"
    fi

    if [[ -n $special ]]; then
        case $special in
            gdpr|debug|port-forward)
                local -r region="$(kubectl config current-context)"
                local -r svcs="$(shipcat list-services -r "$region")"
                COMPREPLY=($(compgen -W "$svcs" -- "$cur"))
                ;;
            get)
                COMPREPLY=($(compgen -W "versions resources images clusterinfo vault-url apistatus codeowners vault-policy" -- "$cur"))
                ;;
            apply|template|values|status|crd|version)
                local -r region="$(kubectl config current-context)"
                local -r svcs="$(shipcat list-services -r "$region")"
                COMPREPLY=($(compgen -W "$svcs" -- "$cur"))
                ;;
            diff)
                local -r region="$(kubectl config current-context)"
                local -r svcs="$(shipcat list-services -r "$region")"
                COMPREPLY=($(compgen -W "$svcs --helm --git --secrets -s --crd" -- "$cur"))
                ;;
            cluster)
                local clustr_sub i
                for (( i=2; i < ${#words[@]}-1; i++ )); do
                    if [[ ${words[i]} = @(crd|kong|vault-policy) ]]; then
                        clustr_sub=${words[i]}
                    fi
                done

                if [[ $prev = "cluster" ]]; then
                    COMPREPLY=($(compgen -W "crd kong vault-policy" -- "$cur"))
                elif [[ $clustr_sub = @(crd|kong) ]]; then
                    # Suggest common verbs
                    COMPREPLY=($(compgen -W "diff reconcile" -- "$cur"))
                fi
                ;;
            config)
                COMPREPLY=($(compgen -W "show verify crd" -- "$cur"))
                ;;
            list-services)
                local -r regions="$(shipcat list-regions)"
                if [[ $prev == @(-r|--region) ]]; then
                    COMPREPLY=($(compgen -W "$region" -- "$cur"))
                else
                    COMPREPLY=($(compgen -W "-r --region" -- "$cur"))
                fi
                ;;
            validate|graph)
                local -r regions="$(shipcat list-regions)"
                if [[ $prev = @(graph|validate) ]]; then
                    svcs=$(find "${mdir}/services" -maxdepth 1 -mindepth 1 -type d -printf "%f " 2> /dev/null)
                    COMPREPLY=($(compgen -W "$svcs -r --region" -- "$cur"))
                elif [[ $prev == @(-r|--region) ]]; then
                    COMPREPLY=($(compgen -W "$regions" -- "$cur"))
                else
                    # Identify which region we used
                    local region i
                    for (( i=2; i < ${#words[@]}-1; i++ )); do
                        if [[ ${words[i]} != -* ]] && echo "$regions" | grep -q "${words[i]}"; then
                            region=${words[i]}
                        fi
                    done
                    local -r svcs="$(shipcat list-services -r "$region")"
                    COMPREPLY=($(compgen -W "$svcs" -- "$cur"))
                fi
                ;;
            shell)
                svcs=$(find "${mdir}/services" -maxdepth 1 -mindepth 1 -type d -printf "%f " 2> /dev/null)
                if [[ $prev = "shell" ]]; then
                    COMPREPLY=($(compgen -W "-r --region -p --pod $svcs" -- "$cur"))
                elif [[ $prev == @(-r|--region) ]]; then
                    local -r regions="$(shipcat list-regions)"
                    COMPREPLY=($(compgen -W "$regions" -- "$cur"))
                elif [[ $prev == @(-p|--pod) ]]; then
                    local -r pods="1 2 3 4 5 6"
                    COMPREPLY=($(compgen -W "$pods" -- "$cur"))
                else
                    COMPREPLY=($(compgen -W "$svcs" -- "$cur"))
                fi
                ;;
        esac
    fi

    return 0
} &&
complete -F _shipcat shipcat
